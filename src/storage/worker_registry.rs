//! wb_worker 存储 —— Worker Registry 持久化（S1 Decision Replay 的前置）。
//! manifest 全量入库：重跑 Router 评分需要 declared 属性（quality/trust/risk/cost…）。

use libsql::params;

use super::*;
use crate::worker_core::WorkerRegistry;
use crate::worker_core::{ExecutionProtocol, WorkerManifest};

impl LibSQLStorage {
    /// 注册/更新 Worker manifest（幂等 upsert：id 主键）。
    pub async fn save_worker(&self, m: &WorkerManifest) -> Result<(), StorageError> {
        let caps = serde_json::to_string(&m.capabilities)?;
        self.conn
            .execute(
                "INSERT OR REPLACE INTO wb_worker \
                 (id, worker_type, capabilities, quality, trust, risk, cost, latency_ms, availability, execution) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![
                    m.id.clone(),
                    m.worker_type.clone(),
                    caps,
                    m.quality,
                    m.trust,
                    m.risk as i64,
                    m.cost as i64,
                    m.latency_ms as i64,
                    m.availability.clone(),
                    format!("{:?}", m.execution),
                ],
            )
            .await?;
        Ok(())
    }

    /// 全部持久化 manifest（Decision Replay 的候选集与 declared 属性）。
    pub async fn list_workers(&self) -> Result<Vec<WorkerManifest>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, worker_type, capabilities, quality, trust, risk, cost, latency_ms, availability, execution \
                 FROM wb_worker ORDER BY id ASC",
                params![],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            let caps: String = row.get(2)?;
            let exec: String = row.get(9)?;
            out.push(WorkerManifest {
                id: row.get(0)?,
                worker_type: row.get(1)?,
                capabilities: serde_json::from_str(&caps)?,
                input_schema: serde_json::json!({}),
                output_schema: serde_json::json!({}),
                execution: match exec.as_str() {
                    "Process" => ExecutionProtocol::Process,
                    "Http" => ExecutionProtocol::Http,
                    "Mcp" => ExecutionProtocol::Mcp,
                    _ => ExecutionProtocol::Embedded,
                },
                cost: row.get(6)?,
                latency_ms: row.get(7)?,
                quality: row.get(3)?,
                risk: row.get(5)?,
                trust: row.get(4)?,
                availability: row.get(8)?,
            });
        }
        Ok(out)
    }

    /// 把运行期 Registry 同步进持久化（预置/注册后调用一次）。
    pub async fn sync_workers(&self, registry: &WorkerRegistry) -> Result<(), StorageError> {
        for (id, _status) in registry.all() {
            if let Some(w) = registry.get(id) {
                self.save_worker(w.manifest()).await?;
            }
        }
        Ok(())
    }
}
