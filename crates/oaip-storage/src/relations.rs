use async_trait::async_trait;
use serde_json::Value;

use super::*;

/// 判断 libsql 错误是否为唯一约束冲突（uq_edge_active 部分唯一索引兜底 link 并发竞态）。
/// libSQL 错误信息形如 "UNIQUE constraint failed: wb_relation_edge.source, ..."。
fn is_unique_violation(e: &libsql::Error) -> bool {
    e.to_string().contains("UNIQUE constraint failed")
}

#[async_trait]
impl RelationEngine for LibSQLStorage {
    async fn link(
        &self,
        source_id: &str,
        edge_type: &str,
        target_id: &str,
        data: Option<Value>,
    ) -> Result<RelationRecord, StorageError> {
        // 并发安全：先事务内 SELECT 去重（旧库无唯一索引时的第一道兜底），
        // 再普通 INSERT——`uq_edge_active` 部分唯一索引（活跃边 is_delete=0 不重复）作硬保证。
        // 注意：libSQL 的 SQLite 不支持 `ON CONFLICT(索引名)` 形式（会报 "no such column"），
        // 且部分唯一索引也无法被 `ON CONFLICT(列列表)` 匹配；故直接 INSERT，
        // 若竞态下另一事务已插入，部分唯一索引会以 UNIQUE constraint failed 拒绝——
        // 此处捕获并转为 WB2007（而不是把底层约束错误泄漏给调用方）。
        let tx = self.conn.transaction().await?;
        let mut dup = tx
            .query(
                "SELECT id FROM wb_relation_edge WHERE source=?1 AND relation=?2 AND target=?3 AND is_delete=0",
                params![source_id, edge_type, target_id],
            )
            .await?;
        if dup.next().await?.is_some() {
            let _ = tx.rollback().await;
            return Err(StorageError::Constraint {
                code: "WB2007",
                message: format!("重复关系边 {source_id}-{edge_type}->{target_id}"),
            });
        }
        let id = format!(
            "rel_{}_{}",
            now_ts(),
            REL_SEQ.fetch_add(1, Ordering::SeqCst)
        );
        let data_str = data.as_ref().map(serde_json::to_string).transpose()?;
        let affected = match tx
            .execute(
                "INSERT INTO wb_relation_edge (id, relation, source, target, data) \
                 VALUES (?1,?2,?3,?4,?5)",
                params![id.clone(), edge_type, source_id, target_id, data_str],
            )
            .await
        {
            Ok(n) => n,
            Err(e) if is_unique_violation(&e) => {
                let _ = tx.rollback().await;
                return Err(StorageError::Constraint {
                    code: "WB2007",
                    message: format!("重复关系边（并发竞态）{source_id}-{edge_type}->{target_id}"),
                });
            }
            Err(e) => return Err(StorageError::Db(e)),
        };
        if affected == 0 {
            let _ = tx.rollback().await;
            return Err(StorageError::Constraint {
                code: "WB2007",
                message: format!("重复关系边（未插入）{source_id}-{edge_type}->{target_id}"),
            });
        }
        tx.commit().await?;
        Ok(RelationRecord {
            id,
            source: source_id.to_string(),
            relation: edge_type.to_string(),
            target: target_id.to_string(),
            data,
        })
    }

    async fn neighbors(&self, source_id: &str) -> Result<Vec<RelationRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, relation, source, target, data FROM wb_relation_edge WHERE source=?1 AND is_delete=0",
                params![source_id],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_relation(&row)?);
        }
        Ok(out)
    }

    async fn query_by_edge(&self, edge_type: &str) -> Result<Vec<RelationRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, relation, source, target, data FROM wb_relation_edge WHERE relation=?1 AND is_delete=0",
                params![edge_type],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_relation(&row)?);
        }
        Ok(out)
    }

    async fn unlink(
        &self,
        source_id: &str,
        edge_type: &str,
        target_id: &str,
    ) -> Result<(), StorageError> {
        self.conn
            .execute(
                "UPDATE wb_relation_edge SET is_delete=1, update_time=CURRENT_TIMESTAMP \
                 WHERE source=?1 AND relation=?2 AND target=?3 AND is_delete=0",
                params![source_id, edge_type, target_id],
            )
            .await?;
        Ok(())
    }
}

// V2 Simulation（S2/S3）：当前世界状态（Relation 集）作为假设投影的输入。
impl LibSQLStorage {
    /// 全部有效关系（is_delete=0；Simulation 的状态输入）。
    pub async fn list_relations(&self) -> Result<Vec<RelationRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, source, relation, target FROM wb_relation_edge WHERE is_delete=0 ORDER BY id ASC",
                params![],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(RelationRecord {
                id: row.get(0)?,
                source: row.get(1)?,
                relation: row.get(2)?,
                target: row.get(3)?,
                data: None,
            });
        }
        Ok(out)
    }
}
