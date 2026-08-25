//! 业务执行租约：崩溃后可识别中断执行，避免盲目重复外部副作用。

use serde::{Deserialize, Serialize};

use libsql::TransactionBehavior;
use libsql::params;

use super::LibSQLStorage;
use super::StorageError;

/// 一条执行租约；`Claimed` 是活跃状态，终态由 Business Loop 显式写入。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionLeaseRecord {
    pub id: String,
    pub project_id: String,
    pub opportunity_id: String,
    pub owner: String,
    pub status: String,
    pub expires_at: String,
}

pub const LEASE_CLAIMED: &str = "Claimed";
pub const LEASE_COMPLETED: &str = "Completed";
pub const LEASE_RELEASED: &str = "Released";
pub const LEASE_EXPIRED: &str = "Expired";

impl LibSQLStorage {
    /// 原子声明执行租约；同一 Opportunity 只允许一条未过期租约。
    pub async fn claim_execution_lease(
        &self,
        id: &str,
        project_id: &str,
        opportunity_id: &str,
        owner: &str,
        expires_at: &str,
    ) -> Result<(), StorageError> {
        let _write_txn = self.write_txn_lock.lock().await;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        let mut rows = tx
            .query(
                "SELECT id FROM wb_business_execution_lease \
                 WHERE opportunity_id=?1 AND status=?2 AND expires_at>?3 LIMIT 1",
                params![opportunity_id, LEASE_CLAIMED, expires_at],
            )
            .await?;
        if rows.next().await?.is_some() {
            return Err(constraint(format!("机会 {opportunity_id} 已有执行租约")));
        }
        drop(rows);
        tx.execute(
            "INSERT INTO wb_business_execution_lease \
             (id, project_id, opportunity_id, owner, status, expires_at) \
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                id,
                project_id,
                opportunity_id,
                owner,
                LEASE_CLAIMED,
                expires_at
            ],
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// 将租约推进到终态；只有当前持有态可完成、释放或过期。
    pub async fn finish_execution_lease(&self, id: &str, status: &str) -> Result<(), StorageError> {
        let updated = self
            .conn
            .execute(
                "UPDATE wb_business_execution_lease SET status=?1, updated_at=CURRENT_TIMESTAMP \
                 WHERE id=?2 AND status=?3",
                params![status, id, LEASE_CLAIMED],
            )
            .await?;
        if updated != 1 {
            return Err(StorageError::NotFound(id.to_string()));
        }
        Ok(())
    }

    /// 将全部过期租约标记为 Expired，并返回给调用方做项目级恢复判定。
    pub async fn expire_execution_leases(
        &self,
        now: &str,
    ) -> Result<Vec<ExecutionLeaseRecord>, StorageError> {
        let _write_txn = self.write_txn_lock.lock().await;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        let mut rows = tx
            .query(
                "SELECT id, project_id, opportunity_id, owner, status, expires_at \
                 FROM wb_business_execution_lease \
                 WHERE status=?1 AND expires_at<=?2 ORDER BY created_at",
                params![LEASE_CLAIMED, now],
            )
            .await?;
        let mut expired = Vec::new();
        while let Some(row) = rows.next().await? {
            expired.push(row_to_lease(&row)?);
        }
        drop(rows);
        for lease in &mut expired {
            tx.execute(
                "UPDATE wb_business_execution_lease SET status=?1, updated_at=CURRENT_TIMESTAMP \
                 WHERE id=?2 AND status=?3",
                params![LEASE_EXPIRED, lease.id.clone(), LEASE_CLAIMED],
            )
            .await?;
            lease.status = LEASE_EXPIRED.into();
        }
        tx.commit().await?;
        Ok(expired)
    }

    /// 列出待人工对账的过期租约；Completed/Released 是正常终态，不进入恢复队列。
    pub async fn list_expired_execution_leases(
        &self,
        project_id: &str,
        limit: u32,
    ) -> Result<Vec<ExecutionLeaseRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, project_id, opportunity_id, owner, status, expires_at \
                 FROM wb_business_execution_lease \
                 WHERE project_id=?1 AND status=?2 \
                 ORDER BY created_at DESC, id DESC LIMIT ?3",
                params![project_id, LEASE_EXPIRED, i64::from(limit.clamp(1, 200))],
            )
            .await?;
        let mut leases = Vec::new();
        while let Some(row) = rows.next().await? {
            leases.push(row_to_lease(&row)?);
        }
        Ok(leases)
    }
}

fn row_to_lease(row: &libsql::Row) -> Result<ExecutionLeaseRecord, StorageError> {
    Ok(ExecutionLeaseRecord {
        id: row.get(0)?,
        project_id: row.get(1)?,
        opportunity_id: row.get(2)?,
        owner: row.get(3)?,
        status: row.get(4)?,
        expires_at: row.get(5)?,
    })
}

fn constraint(message: impl Into<String>) -> StorageError {
    StorageError::Constraint {
        code: "WB2010",
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn lease_db(name: &str) -> (std::path::PathBuf, LibSQLStorage) {
        let path = std::env::temp_dir().join(format!(
            "oaip_{name}_{}_{}.db",
            std::process::id(),
            oaip_trace::now_ts()
        ));
        let _ = std::fs::remove_file(&path);
        let db_path = path.to_str().expect("临时路径应有效");
        let storage = LibSQLStorage::open(db_path).await.expect("数据库应可打开");
        (path, storage)
    }

    #[tokio::test]
    async fn concurrent_claims_allow_only_one_owner() {
        let (path, seed) = lease_db("lease_claim").await;
        let first = seed.clone();
        let second = seed.clone();
        let claim = |storage: LibSQLStorage, owner: &'static str| async move {
            storage
                .claim_execution_lease("lease", "project", "opp-1", owner, "2099-01-01T00:00:00Z")
                .await
        };
        let (left, right) = tokio::join!(claim(first, "first"), claim(second, "second"));
        assert_eq!(
            [left.is_ok(), right.is_ok()]
                .iter()
                .filter(|ok| **ok)
                .count(),
            1,
            "同一机会只能有一个活跃租约"
        );
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn terminal_leases_cannot_finish_again_and_only_expired_leases_expire() {
        let (path, storage) = lease_db("lease_finish").await;
        storage
            .claim_execution_lease("done", "project", "done", "worker", "2099-01-01T00:00:00Z")
            .await
            .expect("租约应可创建");
        storage
            .claim_execution_lease(
                "stale",
                "project",
                "stale",
                "worker",
                "2020-01-01T00:00:00Z",
            )
            .await
            .expect("过期租约应可创建");

        storage
            .finish_execution_lease("done", LEASE_COMPLETED)
            .await
            .expect("活跃租约应能完成");
        assert!(matches!(
            storage.finish_execution_lease("done", LEASE_RELEASED).await,
            Err(StorageError::NotFound(_))
        ));

        let expired = storage
            .expire_execution_leases("2021-01-01T00:00:00Z")
            .await
            .expect("过期扫描应成功");
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].id, "stale");
        assert_eq!(expired[0].status, LEASE_EXPIRED);

        storage
            .claim_execution_lease(
                "other",
                "other-project",
                "other-opportunity",
                "worker",
                "2020-01-01T00:00:00Z",
            )
            .await
            .expect("其他项目过期租约应可创建");
        let leases = storage
            .list_expired_execution_leases("project", 200)
            .await
            .expect("恢复队列应可读取");
        assert_eq!(
            leases
                .iter()
                .map(|lease| lease.id.as_str())
                .collect::<Vec<_>>(),
            ["stale"]
        );
        let _ = std::fs::remove_file(path);
    }
}
