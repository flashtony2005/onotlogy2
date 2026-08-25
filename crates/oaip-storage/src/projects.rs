//! wb_project —— 项目注册表（Phase 8：世界/事件/运营的顶层组织单元）。
//!
//! 项目 = 世界的 scope：对象/关系/事件/run 全部 project 归属；
//! 世界查询按项目过滤投影（事件 → 项目 Kernel Context）。
//! ontology 字段：项目本体定义（ODL 文本，如 OpenAIP-OS 工程本体）。

use serde::{Deserialize, Serialize};

use oaip_abi::{OntologyIR, parse_odl};

use super::{LibSQLStorage, StorageError};

/// 项目记录。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectRecord {
    pub id: String,
    pub name: String,
    pub description: String,
    pub ontology: String,
    pub knowledge_version_id: Option<String>,
    pub status: String,
    pub created_at: String,
}

impl LibSQLStorage {
    async fn resolve_project_knowledge_version(
        &self,
        project_ir: &OntologyIR,
        requested_version_id: Option<&str>,
    ) -> Result<Option<String>, StorageError> {
        let binding_error = |message: String| StorageError::Constraint {
            code: "WB2205",
            message,
        };
        let Some(version) = (match requested_version_id {
            Some(id) => Some(
                self.knowledge_version(id)
                    .await?
                    .ok_or_else(|| StorageError::NotFound(id.to_string()))?,
            ),
            None => self.current_knowledge_version().await?,
        }) else {
            return Ok(None);
        };
        let Some(runtime_ir) = version.runtime_ir.as_ref() else {
            return Err(binding_error(format!(
                "本体版本 {} 缺少可执行 Runtime IR",
                version.id
            )));
        };
        if runtime_ir != project_ir {
            return Err(binding_error(format!(
                "项目 ODL 与本体版本 {} 的可执行投影不一致",
                version.id
            )));
        }
        Ok(Some(version.id))
    }

    pub async fn bind_project_knowledge_version(
        &self,
        project_id: &str,
        version_id: &str,
    ) -> Result<(), StorageError> {
        let _write_txn = self.write_txn_lock.lock().await;
        if self.project(project_id).await?.is_none() {
            return Err(StorageError::NotFound(project_id.to_string()));
        }
        let tx = self.conn.transaction().await?;
        let Some(runtime_ir) = self
            .knowledge_version(version_id)
            .await?
            .ok_or_else(|| StorageError::NotFound(version_id.to_string()))?
            .runtime_ir
        else {
            return Err(StorageError::Constraint {
                code: "WB2205",
                message: format!("本体版本 {version_id} 缺少可执行 Runtime IR"),
            });
        };
        crate::ontology::validate_runtime_ir_against_world(&tx, &runtime_ir).await?;
        let updated = tx
            .execute(
                "UPDATE wb_project SET knowledge_version_id=?1 WHERE id=?2",
                libsql::params![version_id, project_id],
            )
            .await?;
        if updated == 0 {
            return Err(StorageError::NotFound(project_id.to_string()));
        }
        tx.commit().await?;
        Ok(())
    }

    /// 创建项目（id 由调用方持有语义 id，如 "openaip-os"）。
    pub async fn create_project(
        &self,
        id: &str,
        name: &str,
        description: &str,
        ontology: &str,
        knowledge_version_id: Option<&str>,
    ) -> Result<ProjectRecord, StorageError> {
        let project_ir = parse_odl(ontology)?;
        let bound_version_id = self
            .resolve_project_knowledge_version(&project_ir, knowledge_version_id)
            .await?;
        self.conn
            .execute(
                "INSERT INTO wb_project (id, name, description, ontology, knowledge_version_id) \
                 VALUES (?1,?2,?3,?4,?5)",
                libsql::params![id, name, description, ontology, bound_version_id],
            )
            .await?;
        Ok(self.project(id).await?.expect("刚插入必在"))
    }

    /// 按 id 查项目。
    pub async fn project(&self, id: &str) -> Result<Option<ProjectRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, name, description, ontology, knowledge_version_id, status, created_at \
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
            knowledge_version_id: row.get(4)?,
            status: row.get(5)?,
            created_at: row.get(6)?,
        }))
    }

    /// 列出全部项目。
    pub async fn list_projects(&self) -> Result<Vec<ProjectRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, name, description, ontology, knowledge_version_id, status, created_at \
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
                knowledge_version_id: row.get(4)?,
                status: row.get(5)?,
                created_at: row.get(6)?,
            });
        }
        Ok(out)
    }
}
