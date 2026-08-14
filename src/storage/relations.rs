use async_trait::async_trait;
use serde_json::Value;

use super::*;

#[async_trait]
impl RelationEngine for LibSQLStorage {
    async fn link(
        &self,
        source_id: &str,
        edge_type: &str,
        target_id: &str,
        data: Option<Value>,
    ) -> Result<RelationRecord, StorageError> {
        // 事务内「检测 + 插入」：两个并发 link 同一对边时，重复检测与插入原子，
        // 杜绝 TOCTOU 双插（检查通过后另一事务已插入 → 唯一性由事务串行保证）。
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
        tx.execute(
            "INSERT INTO wb_relation_edge (id, relation, source, target, data) VALUES (?1,?2,?3,?4,?5)",
            params![id.clone(), edge_type, source_id, target_id, data_str],
        )
        .await?;
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
