use super::*;

impl LibSQLStorage {
    /// 沉淀一条记忆：按 `label` upsert；已存在则累加 `hits` 并更新 `payload`，实现经验复用计数。
    pub async fn record_memory(
        &self,
        label: &str,
        payload: Value,
    ) -> Result<MemoryRecord, StorageError> {
        let payload_s = serde_json::to_string(&payload)?;
        self.conn
            .execute(
                "INSERT INTO wb_memory (label, payload, hits, created_at, updated_at) \
                 VALUES (?1, ?2, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP) \
                 ON CONFLICT(label) DO UPDATE SET \
                   hits = hits + 1, payload = ?2, updated_at = CURRENT_TIMESTAMP",
                params![label, payload_s],
            )
            .await?;
        self.recall_memory(label)
            .await?
            .ok_or_else(|| StorageError::NotFound(format!("memory:{label}")))
    }

    /// 按标签召回单条记忆（缺失 → None）。
    pub async fn recall_memory(&self, label: &str) -> Result<Option<MemoryRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT label, payload, hits, created_at, updated_at FROM wb_memory WHERE label = ?1",
                params![label],
            )
            .await?;
        match rows.next().await? {
            Some(row) => Ok(Some(row_to_memory(&row)?)),
            None => Ok(None),
        }
    }

    /// 召回全部记忆（按 hits 降序，复用频次高的在前）。
    pub async fn recall_all(&self) -> Result<Vec<MemoryRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT label, payload, hits, created_at, updated_at FROM wb_memory \
                 ORDER BY hits DESC, updated_at DESC LIMIT 1000",
                params![],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_memory(&row)?);
        }
        Ok(out)
    }
}
