//! wb_project —— 项目注册表（Phase 8：世界/事件/运营的顶层组织单元）。
//!
//! 项目 = 世界的 scope：对象/关系/事件/run 全部 project 归属；
//! 世界查询按项目过滤投影（事件 → 项目 Kernel Context）。
//! ontology 字段：项目本体定义（ODL 文本，如 OpenAIP-OS 工程本体）。

use serde::{Deserialize, Serialize};

use super::{LibSQLStorage, StorageError};

/// 项目记录。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectRecord {
    pub id: String,
    pub name: String,
    pub description: String,
    pub ontology: String,
    pub status: String,
    pub created_at: String,
}

impl LibSQLStorage {
    /// 创建项目（id 由调用方持有语义 id，如 "openaip-os"）。
    pub async fn create_project(
        &self,
        id: &str,
        name: &str,
        description: &str,
        ontology: &str,
    ) -> Result<ProjectRecord, StorageError> {
        self.conn
            .execute(
                "INSERT INTO wb_project (id, name, description, ontology) VALUES (?1,?2,?3,?4)",
                libsql::params![id, name, description, ontology],
            )
            .await?;
        Ok(self.project(id).await?.expect("刚插入必在"))
    }

    /// 按 id 查项目。
    pub async fn project(&self, id: &str) -> Result<Option<ProjectRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, name, description, ontology, status, created_at \
                 FROM wb_project WHERE id = ?1",
                libsql::params![id],
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Ok(None);
        };
        Ok(Some(ProjectRecord {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            ontology: row.get(3)?,
            status: row.get(4)?,
            created_at: row.get(5)?,
        }))
    }

    /// 列出全部项目。
    pub async fn list_projects(&self) -> Result<Vec<ProjectRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, name, description, ontology, status, created_at \
                 FROM wb_project ORDER BY created_at ASC",
                libsql::params![],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(ProjectRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                ontology: row.get(3)?,
                status: row.get(4)?,
                created_at: row.get(5)?,
            });
        }
        Ok(out)
    }
}
