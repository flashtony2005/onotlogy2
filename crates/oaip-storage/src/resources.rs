//! 企业共享资源账本：跨项目、跨实例的预算仲裁事实源。
//!
//! 写入使用 IMMEDIATE 事务，在数据库层串行化“读取已用 + 校验限额 + 插入账本”；
//! 进程内写锁只负责避免同一 libSQL 连接重入事务。

use std::collections::BTreeMap;

use libsql::TransactionBehavior;
use libsql::params;

use super::LibSQLStorage;
use super::StorageError;

impl LibSQLStorage {
    /// 原子预留一批资源。`limits` 只约束声明为共享的维度；其他维度仅入账。
    pub async fn reserve_shared_resources(
        &self,
        id: &str,
        project_id: &str,
        resources: &BTreeMap<String, f64>,
        limits: &BTreeMap<String, f64>,
        cutoff: Option<&str>,
        executed_at: &str,
    ) -> Result<(), StorageError> {
        let _write_txn = self.write_txn_lock.lock().await;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        for (name, &amount) in resources {
            if !amount.is_finite() || amount < 0.0 {
                return Err(constraint(format!("共享资源 {name} 数量必须是非负有限值")));
            }
            if let Some(limit) = limits.get(name) {
                let mut rows = tx
                    .query(
                        "SELECT COALESCE(SUM(CAST(amount AS REAL)), 0.0) FROM wb_shared_resource_usage \
                         WHERE resource_name = ?1 \
                           AND (?2 IS NULL OR executed_at >= ?2)",
                        params![name.as_str(), cutoff],
                    )
                    .await?;
                let spent = match rows.next().await? {
                    Some(row) => row.get::<f64>(0)?,
                    None => 0.0,
                };
                drop(rows);
                if spent + amount > *limit {
                    return Err(constraint(format!(
                        "共享资源 {name} 超出剩余预算 {:.0}",
                        (limit - spent).max(0.0)
                    )));
                }
            }
            tx.execute(
                "INSERT INTO wb_shared_resource_usage \
                 (id, project_id, resource_name, amount, executed_at) VALUES (?1,?2,?3,?4,?5)",
                params![id, project_id, name.as_str(), amount, executed_at],
            )
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// 按窗口聚合企业级资源消耗；`cutoff=None` 表示生命周期累计。
    pub async fn shared_resource_usage(
        &self,
        cutoff: Option<&str>,
    ) -> Result<BTreeMap<String, f64>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT resource_name, SUM(CAST(amount AS REAL)) FROM wb_shared_resource_usage \
                 WHERE (?1 IS NULL OR executed_at >= ?1) GROUP BY resource_name",
                params![cutoff],
            )
            .await?;
        let mut usage = BTreeMap::new();
        while let Some(row) = rows.next().await? {
            usage.insert(row.get::<String>(0)?, row.get::<f64>(1)?);
        }
        Ok(usage)
    }
}

fn constraint(message: impl Into<String>) -> StorageError {
    StorageError::Constraint {
        code: "WB2008",
        message: message.into(),
    }
}
