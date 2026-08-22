use async_trait::async_trait;
use serde_json::Value;

use super::*;

impl LibSQLStorage {
    pub async fn list_all_objects(&self) -> Result<Vec<ObjectRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, type, data, version FROM wb_ontology_object WHERE is_delete=0",
                params![],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_object(&row)?);
        }
        Ok(out)
    }

    /// 按项目和对象类型列出对象。业务包运行配置、策略注册表等
    /// 复用世界对象与审计链，不新增业务专用表。
    pub async fn list_objects_for_project(
        &self,
        project_id: &str,
        object_type: &str,
    ) -> Result<Vec<ObjectRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, type, data, version FROM wb_ontology_object WHERE project_id=?1 AND type=?2 AND is_delete=0 ORDER BY id",
                params![project_id, object_type],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_object(&row)?);
        }
        Ok(out)
    }

    /// 列出全部关系边（is_delete=0），供世界模型快照。
    pub async fn list_all_relations(&self) -> Result<Vec<RelationRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, relation, source, target, data FROM wb_relation_edge WHERE is_delete=0 LIMIT 1000",
                params![],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_relation(&row)?);
        }
        Ok(out)
    }

    /// 渲染对象列表。客户：`id name level`（TC-006）；任务：`id title status`。
    pub fn render_object_list(records: &[ObjectRecord]) -> String {
        let mut lines = Vec::new();
        for r in records {
            if r.data.get("status").is_some() {
                let title = r.data.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let status = r.data.get("status").and_then(|v| v.as_str()).unwrap_or("");
                lines.push(format!("{} {} {}", r.id, title, status));
            } else {
                let name = r.data.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let level = r.data.get("level").and_then(|v| v.as_str()).unwrap_or("");
                lines.push(format!("{} {} {}", r.id, name, level));
            }
        }
        lines.join("\n")
    }
}

#[async_trait]
impl ObjectEngine for LibSQLStorage {
    async fn create(
        &self,
        object_type: &str,
        id: &str,
        data: Value,
    ) -> Result<ObjectRecord, StorageError> {
        let data_str = serde_json::to_string(&data)?;
        self.conn
            .execute(
                "INSERT INTO wb_ontology_object (id, type, data, version) VALUES (?1,?2,?3,1)",
                params![id, object_type, data_str],
            )
            .await?;
        Ok(ObjectRecord {
            id: id.to_string(),
            r#type: object_type.to_string(),
            data,
            version: 1,
        })
    }

    async fn get(&self, id: &str) -> Result<Option<ObjectRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, type, data, version FROM wb_ontology_object WHERE id=?1 AND is_delete=0",
                params![id],
            )
            .await?;
        if let Some(row) = rows.next().await? {
            Ok(Some(row_to_object(&row)?))
        } else {
            Ok(None)
        }
    }

    async fn update(
        &self,
        id: &str,
        data: Value,
        expected_version: i64,
    ) -> Result<ObjectRecord, StorageError> {
        let data_str = serde_json::to_string(&data)?;
        // 乐观锁：WHERE id AND version=? —— 仅当客户端读到的版本仍是最新时才写入，
        // version=version+1 原子自增。两并发更新：后到者 version 不符 → 影响 0 行 → 冲突。
        // （旧实现仅靠 version=version+1 盲目自增，无法检测丢失更新，注释声称的防护是错的。）
        let n = self
            .conn
            .execute(
                "UPDATE wb_ontology_object SET data=?1, version=version+1, \
                 update_time=CURRENT_TIMESTAMP WHERE id=?2 AND version=?3 AND is_delete=0",
                params![data_str, id, expected_version],
            )
            .await?;
        if n == 0 {
            // 0 行：要么对象不存在（is_delete=1 / id 错），要么版本不符（乐观锁冲突）。
            // 用 get 区分：能读到非删除行说明是版本冲突，否则是不存在。
            let exists = self
                .conn
                .query(
                    "SELECT id FROM wb_ontology_object WHERE id=?1 AND is_delete=0",
                    params![id],
                )
                .await?
                .next()
                .await?
                .is_some();
            if exists {
                return Err(StorageError::Conflict {
                    id: id.to_string(),
                    expected: expected_version,
                });
            }
            return Err(StorageError::NotFound(id.to_string()));
        }
        // 读回最新行（含原子递增后的 version）
        let mut rows = self
            .conn
            .query(
                "SELECT id, type, data, version FROM wb_ontology_object WHERE id=?1 AND is_delete=0",
                params![id],
            )
            .await?;
        let row = rows
            .next()
            .await?
            .ok_or_else(|| StorageError::NotFound(id.to_string()))?;
        row_to_object(&row)
    }

    async fn query(&self, object_type: &str) -> Result<Vec<ObjectRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, type, data, version FROM wb_ontology_object WHERE type=?1 AND is_delete=0 LIMIT 1000",
                params![object_type],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_object(&row)?);
        }
        Ok(out)
    }

    async fn delete(&self, id: &str) -> Result<(), StorageError> {
        let n = self
            .conn
            .execute(
                "UPDATE wb_ontology_object SET is_delete=1, update_time=CURRENT_TIMESTAMP WHERE id=?1",
                params![id],
            )
            .await?;
        if n == 0 {
            return Err(StorageError::NotFound(id.to_string()));
        }
        Ok(())
    }
}

impl LibSQLStorage {
    /// 硬删除（物理删行）。仅供演示/CLI 重灌，产品路径用 `ObjectEngine::delete` 软删除。
    pub async fn hard_delete(&self, id: &str) -> Result<u64, StorageError> {
        let n = self
            .conn
            .execute("DELETE FROM wb_ontology_object WHERE id=?1", params![id])
            .await?;
        Ok(n)
    }
}

// ── 运行态持久化：wb_run（审批状态由 Kernel/AppState 管理） ──
