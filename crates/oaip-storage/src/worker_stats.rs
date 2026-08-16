//! wb_worker_stats 存储 —— V2 Router Selection Learning（Trust 轴）派生层。
//! (capability, worker) 执行统计：幂等累积，可回放重建。

use libsql::params;

use super::*;
use oaip_worker_core::stats::WorkerStat;

impl LibSQLStorage {
    /// 记录一次 worker 执行（upsert 累积：runs+1，success 按成败）。
    pub async fn record_worker_outcome(
        &self,
        capability: &str,
        worker_id: &str,
        ok: bool,
        duration_ms: u64,
    ) -> Result<(), StorageError> {
        self.conn
            .execute(
                "INSERT INTO wb_worker_stats (capability, worker_id, runs, success, total_duration_ms) \
                 VALUES (?1,?2,1,?3,?4) \
                 ON CONFLICT(capability, worker_id) DO UPDATE SET \
                   runs = runs + 1, \
                   success = success + ?3, \
                   total_duration_ms = total_duration_ms + ?4, \
                   updated_at = CURRENT_TIMESTAMP",
                params![
                    capability,
                    worker_id,
                    if ok { 1 } else { 0 },
                    duration_ms as i64
                ],
            )
            .await?;
        Ok(())
    }

    /// 清空统计（rebuild_from_evals 全量重建的第一步）。
    pub async fn clear_worker_stats(&self) -> Result<(), StorageError> {
        self.conn
            .execute("DELETE FROM wb_worker_stats", params![])
            .await?;
        Ok(())
    }

    /// 全部统计（学习 map 的数据源）。
    pub async fn list_worker_stats(&self) -> Result<Vec<WorkerStat>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT capability, worker_id, runs, success, total_duration_ms, created_at, updated_at \
                 FROM wb_worker_stats ORDER BY capability ASC, worker_id ASC",
                params![],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(WorkerStat {
                capability: row.get(0)?,
                worker_id: row.get(1)?,
                runs: row.get(2)?,
                success: row.get(3)?,
                total_duration_ms: row.get(4)?,
                policy_compliance: None,
                avg_cost: None,
                first_seen: Some(row.get(5)?),
                last_seen: Some(row.get(6)?),
            });
        }
        Ok(out)
    }
}
