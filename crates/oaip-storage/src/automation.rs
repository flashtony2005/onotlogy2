//! 自动运行引擎的持久化任务队列。
//!
//! 任务队列属于宿主运行时，而不是某个业务包。它只保存调度事实和结果，
//! 业务能力由上层插件目录执行。第一版约束为单进程、单 worker、串行领取。

use libsql::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{LibSQLStorage, StorageError};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AutomationJob {
    pub id: String,
    pub pack_id: String,
    pub job_type: String,
    pub dedupe_key: String,
    pub payload: Value,
    pub status: String,
    pub attempt: i64,
    pub max_attempts: i64,
    pub available_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub last_error: Option<String>,
    pub result: Option<Value>,
    pub created_at: String,
    pub updated_at: String,
}

fn now_ts() -> String {
    oaip_trace::now_ts()
}

fn row_to_job(row: &libsql::Row) -> Result<AutomationJob, StorageError> {
    let payload: String = row.get(4)?;
    let result: Option<String> = row.get(12)?;
    Ok(AutomationJob {
        id: row.get(0)?,
        pack_id: row.get(1)?,
        job_type: row.get(2)?,
        dedupe_key: row.get(3)?,
        payload: serde_json::from_str(&payload)?,
        status: row.get(5)?,
        attempt: row.get(6)?,
        max_attempts: row.get(7)?,
        available_at: row.get(8)?,
        started_at: row.get(9)?,
        finished_at: row.get(10)?,
        last_error: row.get(11)?,
        result: result
            .map(|value| serde_json::from_str(&value))
            .transpose()?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

const JOB_COLUMNS: &str = "id, pack_id, job_type, dedupe_key, payload, status, attempt, max_attempts, available_at, started_at, finished_at, last_error, result, created_at, updated_at";

fn is_unique_violation(error: &libsql::Error) -> bool {
    error.to_string().contains("UNIQUE constraint failed")
}

impl LibSQLStorage {
    /// 幂等入队。相同业务包、任务类型和去重键只产生一个任务。
    pub async fn enqueue_automation_job(
        &self,
        pack_id: &str,
        job_type: &str,
        dedupe_key: &str,
        payload: &Value,
        max_attempts: i64,
    ) -> Result<(AutomationJob, bool), StorageError> {
        if pack_id.trim().is_empty() || job_type.trim().is_empty() || dedupe_key.trim().is_empty() {
            return Err(StorageError::Constraint {
                code: "WB2110",
                message: "自动化任务的 pack_id、job_type、dedupe_key 不能为空".into(),
            });
        }
        let now = now_ts();
        let id = format!("automation:{pack_id}:{job_type}:{dedupe_key}");
        let insert = self
            .conn
            .execute(
                "INSERT INTO wb_automation_job \
                 (id, pack_id, job_type, dedupe_key, payload, status, attempt, max_attempts, available_at, created_at, updated_at) \
                 VALUES (?1,?2,?3,?4,?5,'queued',0,?6,?7,?7,?7)",
                params![
                    id.clone(),
                    pack_id,
                    job_type,
                    dedupe_key,
                    serde_json::to_string(payload)?,
                    max_attempts.max(1),
                    now.clone()
                ],
            )
            .await
            ;
        let inserted = match insert {
            Ok(inserted) => inserted,
            Err(error) if is_unique_violation(&error) => {
                let text = error.to_string();
                if !text.contains("uq_automation_active_business_optimization_project") {
                    return Ok((self.get_automation_job(&id).await?.unwrap(), false));
                }
                return Err(StorageError::Constraint {
                    code: "WB2112",
                    message: "活跃业务任务违反项目唯一约束".into(),
                });
            }
            Err(error) => return Err(StorageError::Db(error)),
        };
        let job = self
            .get_automation_job(&id)
            .await?
            .ok_or_else(|| StorageError::Constraint {
                code: "WB2111",
                message: "自动化任务入队后无法读取任务".into(),
            })?;
        Ok((job, inserted > 0))
    }

    pub async fn get_automation_job(
        &self,
        id: &str,
    ) -> Result<Option<AutomationJob>, StorageError> {
        let mut rows = self
            .conn
            .query(
                &format!("SELECT {JOB_COLUMNS} FROM wb_automation_job WHERE id=?1"),
                params![id],
            )
            .await?;
        rows.next().await?.map(|row| row_to_job(&row)).transpose()
    }

    pub async fn list_automation_jobs(
        &self,
        pack_id: Option<&str>,
        limit: u32,
    ) -> Result<Vec<AutomationJob>, StorageError> {
        let limit = i64::from(limit.clamp(1, 200));
        let (sql, args): (String, Vec<libsql::Value>) = match pack_id {
            Some(pack_id) => (
                format!(
                    "SELECT {JOB_COLUMNS} FROM wb_automation_job WHERE pack_id=?1 ORDER BY created_at DESC, id DESC LIMIT ?2"
                ),
                vec![pack_id.into(), limit.into()],
            ),
            None => (
                format!(
                    "SELECT {JOB_COLUMNS} FROM wb_automation_job ORDER BY created_at DESC, id DESC LIMIT ?1"
                ),
                vec![limit.into()],
            ),
        };
        let mut rows = self.conn.query(&sql, args).await?;
        let mut jobs = Vec::new();
        while let Some(row) = rows.next().await? {
            jobs.push(row_to_job(&row)?);
        }
        Ok(jobs)
    }

    /// 恢复上次进程崩溃留下的 running 任务，避免永久卡死。
    pub async fn recover_automation_jobs(&self) -> Result<u64, StorageError> {
        let now = now_ts();
        let _write_txn = self.write_txn_lock.lock().await;
        let tx = self.conn.transaction().await?;
        let failed = tx
            .execute(
                "UPDATE wb_automation_job SET status='failed', finished_at=?1, last_error='interrupted after max attempts', updated_at=?1 WHERE status='running' AND attempt >= max_attempts",
                params![now.clone()],
            )
            .await?;
        tx.execute(
            "UPDATE wb_automation_job SET status='queued', available_at=?1, last_error='interrupted by previous process', updated_at=?1 WHERE status='running' AND attempt < max_attempts",
            params![now],
        )
        .await?;
        tx.commit().await?;
        Ok(failed)
    }

    /// 在事务内领取最早可执行任务。单 worker 仍使用事务，确保 HTTP 手动触发不会重复领取。
    pub async fn claim_next_automation_job(&self) -> Result<Option<AutomationJob>, StorageError> {
        let now = now_ts();
        let _write_txn = self.write_txn_lock.lock().await;
        let tx = self.conn.transaction().await?;
        let mut rows = tx
            .query(
                &format!(
                    "SELECT {JOB_COLUMNS} FROM wb_automation_job WHERE status='queued' AND available_at<=?1 ORDER BY available_at ASC, created_at ASC, id ASC LIMIT 1"
                ),
                params![now.clone()],
            )
            .await?;
        let Some(row) = rows.next().await? else {
            drop(rows);
            tx.commit().await?;
            return Ok(None);
        };
        let job = row_to_job(&row)?;
        drop(rows);
        tx.execute(
            "UPDATE wb_automation_job SET status='running', attempt=attempt+1, started_at=?1, updated_at=?1 WHERE id=?2 AND status='queued'",
            params![now.clone(), job.id.clone()],
        )
        .await?;
        tx.commit().await?;
        self.get_automation_job(&job.id).await
    }

    pub async fn complete_automation_job(
        &self,
        id: &str,
        result: &Value,
    ) -> Result<(), StorageError> {
        let now = now_ts();
        let affected = self
            .conn
            .execute(
                "UPDATE wb_automation_job SET status='succeeded', finished_at=?1, result=?2, last_error=NULL, updated_at=?1 WHERE id=?3 AND status='running'",
                params![now, serde_json::to_string(result)?, id],
            )
            .await?;
        if affected != 1 {
            return Err(StorageError::NotFound(format!("automation_job:{id}")));
        }
        Ok(())
    }

    pub async fn fail_automation_job(
        &self,
        id: &str,
        error: &str,
        retry_delay_seconds: u64,
    ) -> Result<(), StorageError> {
        let Some(job) = self.get_automation_job(id).await? else {
            return Err(StorageError::NotFound(format!("automation_job:{id}")));
        };
        let now = now_ts();
        let retry = job.attempt < job.max_attempts;
        let available_at = if retry {
            let now_micros = now.parse::<u64>().unwrap_or_default();
            now_micros
                .saturating_add(retry_delay_seconds.saturating_mul(1_000_000))
                .to_string()
        } else {
            now.clone()
        };
        let affected = self
            .conn
            .execute(
                "UPDATE wb_automation_job SET status=?1, available_at=?2, finished_at=?3, last_error=?4, updated_at=?6 WHERE id=?5 AND status='running'",
                params![
                    if retry { "queued" } else { "failed" },
                    available_at,
                    if retry { Option::<String>::None } else { Some(now.clone()) },
                    error,
                    id,
                    now
                ],
            )
            .await?;
        if affected != 1 {
            return Err(StorageError::NotFound(format!("automation_job:{id}")));
        }
        Ok(())
    }

    pub async fn cancel_automation_job(&self, id: &str) -> Result<(), StorageError> {
        let now = now_ts();
        let affected = self
            .conn
            .execute(
                "UPDATE wb_automation_job SET status='cancelled', finished_at=?1, updated_at=?1 WHERE id=?2 AND status IN ('queued','running')",
                params![now, id],
            )
            .await?;
        if affected != 1 {
            return Err(StorageError::NotFound(format!("automation_job:{id}")));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn jobs_are_deduplicated_and_claimed() {
        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        let payload = serde_json::json!({"file":"/tmp/a.csv"});
        let (first, inserted) = storage
            .enqueue_automation_job("lottery-analysis", "sync", "fp-1", &payload, 3)
            .await
            .unwrap();
        assert!(inserted);
        let (same, inserted) = storage
            .enqueue_automation_job("lottery-analysis", "sync", "fp-1", &payload, 3)
            .await
            .unwrap();
        assert!(!inserted);
        assert_eq!(first.id, same.id);
        let claimed = storage.claim_next_automation_job().await.unwrap().unwrap();
        assert_eq!(claimed.status, "running");
        assert_eq!(claimed.attempt, 1);
        storage
            .complete_automation_job(&claimed.id, &serde_json::json!({"ok":true}))
            .await
            .unwrap();
        assert_eq!(
            storage
                .get_automation_job(&claimed.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            "succeeded"
        );
    }

    #[tokio::test]
    async fn active_business_optimization_projects_are_unique_across_instances() {
        let shared = LibSQLStorage::open(":memory:").await.unwrap();
        let first = LibSQLStorage::from_connection(shared.conn.clone());
        let second = LibSQLStorage::from_connection(shared.conn.clone());
        let payload = serde_json::json!({"projectId": "crm-retention"});

        let (job, created) = first
            .enqueue_automation_job("business-loop", "optimize", "first-instance", &payload, 1)
            .await
            .unwrap();
        assert!(created);

        let duplicate = second
            .enqueue_automation_job("business-loop", "optimize", "second-instance", &payload, 1)
            .await
            .unwrap_err();
        let StorageError::Constraint { code, .. } = duplicate else {
            panic!("第二个实例必须被数据库唯一索引拒绝: {duplicate:?}");
        };
        assert_eq!(code, "WB2112");

        second.cancel_automation_job(&job.id).await.unwrap();
        let requeued = second
            .enqueue_automation_job(
                "business-loop",
                "optimize",
                "after-terminal-status",
                &payload,
                1,
            )
            .await
            .unwrap();
        assert!(requeued.1);
    }

    #[tokio::test]
    async fn failed_jobs_retry_then_stop_at_max_attempts() {
        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        let (job, _) = storage
            .enqueue_automation_job("pack", "sync", "fp", &serde_json::json!({}), 2)
            .await
            .unwrap();
        let claimed = storage.claim_next_automation_job().await.unwrap().unwrap();
        storage
            .fail_automation_job(&claimed.id, "first", 0)
            .await
            .unwrap();
        let retry = storage.claim_next_automation_job().await.unwrap().unwrap();
        assert_eq!(retry.attempt, 2);
        storage
            .fail_automation_job(&retry.id, "second", 0)
            .await
            .unwrap();
        assert_eq!(
            storage
                .get_automation_job(&job.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            "failed"
        );
    }

    #[tokio::test]
    async fn cancellation_is_terminal() {
        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        let (job, _) = storage
            .enqueue_automation_job("pack", "sync", "cancel", &serde_json::json!({}), 1)
            .await
            .unwrap();
        storage.cancel_automation_job(&job.id).await.unwrap();
        assert_eq!(
            storage
                .get_automation_job(&job.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            "cancelled"
        );
        assert!(
            storage
                .complete_automation_job(&job.id, &serde_json::json!({}))
                .await
                .is_err()
        );
        assert!(storage.cancel_automation_job(&job.id).await.is_err());
    }
}
