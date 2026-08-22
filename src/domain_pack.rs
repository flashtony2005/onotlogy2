//! 公司业务包：把一套可执行本体、初始业务设置和种子事实作为一个发布单元。
//!
//! 业务包不是第二套存储模型：本体仍由 ODL 描述，企业设置和初始业务事实仍写入
//! `wb_ontology_object` / `wb_relation_edge`，并通过 `WorldCommand` 留下项目审计事件。

use crate::abi::ir::OntologyIR;
use crate::abi::odl::parse_odl;
use crate::abi::validate_object_data;
use crate::storage::{LibSQLStorage, ObjectEngine, StorageError, WorldCommand};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DomainPackManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub ontology: String,
    pub bootstrap: String,
    #[serde(rename = "minimumRuntime")]
    pub minimum_runtime: Option<String>,
    /// 业务包声明所需的操作 Skill；空值表示不绑定特定 Skill。
    #[serde(default)]
    pub skills: Vec<String>,
}

/// 业务包运行时配置。配置属于业务包域模型，HTTP 只是其管理入口；
/// 自动运行引擎直接消费同一结构，避免出现两套配置定义。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DomainPackRuntime {
    pub pack_id: String,
    pub enabled: bool,
    pub source_path: String,
    pub watch_enabled: bool,
    pub poll_interval_seconds: u64,
    pub auto_sync: bool,
    pub auto_analyze: bool,
    pub auto_backtest: bool,
    pub auto_review: bool,
    pub strategy_ids: Vec<String>,
    pub updated_by: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BootstrapObject {
    pub id: String,
    #[serde(rename = "type")]
    pub object_type: String,
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BootstrapRelation {
    pub source: String,
    pub relation: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct BootstrapData {
    #[serde(default)]
    pub objects: Vec<BootstrapObject>,
    #[serde(default)]
    pub relations: Vec<BootstrapRelation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallReport {
    pub project_id: String,
    pub objects_created: usize,
    pub objects_skipped: usize,
    pub relations_created: usize,
    pub relations_skipped: usize,
}

#[derive(Debug, Error)]
pub enum DomainPackError {
    #[error("业务包文件错误：{0}")]
    Io(#[from] std::io::Error),
    #[error("业务包 JSON 错误：{0}")]
    Json(#[from] serde_json::Error),
    #[error("业务包 ODL 错误：{0}")]
    Odl(#[from] crate::abi::odl::OdlError),
    #[error("业务包无效：{0}")]
    Invalid(String),
    #[error("业务包安装失败：{0}")]
    Storage(#[from] StorageError),
}

#[derive(Debug, Clone)]
pub struct DomainPack {
    root: PathBuf,
    pub manifest: DomainPackManifest,
    pub ontology_source: String,
    pub ontology: OntologyIR,
    pub bootstrap: BootstrapData,
}

impl DomainPack {
    pub fn load(root: impl AsRef<Path>) -> Result<Self, DomainPackError> {
        let root = root.as_ref().to_path_buf();
        if !root.is_dir() {
            return Err(DomainPackError::Invalid(format!(
                "业务包目录不存在：{}",
                root.display()
            )));
        }
        let manifest: DomainPackManifest = read_json(&root.join("manifest.json"))?;
        validate_identifier("id", &manifest.id)?;
        if manifest.name.trim().is_empty() || manifest.version.trim().is_empty() {
            return Err(DomainPackError::Invalid("name/version 不能为空".into()));
        }
        let ontology_path = safe_child(&root, &manifest.ontology)?;
        let bootstrap_path = safe_child(&root, &manifest.bootstrap)?;
        let ontology_source = std::fs::read_to_string(ontology_path)?;
        let ontology = parse_odl(&ontology_source)?;
        let bootstrap: BootstrapData = read_json(&bootstrap_path)?;
        let pack = Self {
            root,
            manifest,
            ontology_source,
            ontology,
            bootstrap,
        };
        pack.validate()?;
        Ok(pack)
    }

    pub fn validate(&self) -> Result<(), DomainPackError> {
        let mut ids = HashSet::new();
        for object in &self.bootstrap.objects {
            if object.id.trim().is_empty() || !ids.insert(object.id.clone()) {
                return Err(DomainPackError::Invalid(format!(
                    "对象 ID 为空或重复：{}",
                    object.id
                )));
            }
            let Some(definition) = self.ontology.find(&object.object_type) else {
                return Err(DomainPackError::Invalid(format!(
                    "bootstrap 对象类型未在 ODL 中定义：{}",
                    object.object_type
                )));
            };
            validate_object_data(definition, &object.data)
                .map_err(|e| DomainPackError::Invalid(format!("对象 {}：{e}", object.id)))?;
        }
        for relation in &self.bootstrap.relations {
            if !ids.contains(&relation.source) || !ids.contains(&relation.target) {
                return Err(DomainPackError::Invalid(format!(
                    "关系引用了未在 bootstrap 中声明的对象：{} -{}-> {}",
                    relation.source, relation.relation, relation.target
                )));
            }
            if relation.relation.trim().is_empty() {
                return Err(DomainPackError::Invalid("关系名称不能为空".into()));
            }
        }
        Ok(())
    }

    pub async fn install(&self, storage: &LibSQLStorage) -> Result<InstallReport, DomainPackError> {
        self.validate()?;
        let project_id = &self.manifest.id;
        if let Some(project) = storage.project(project_id).await? {
            if project.ontology != self.ontology_source {
                return Err(DomainPackError::Invalid(format!(
                    "项目 {} 已存在但本体内容不同；请使用新的业务包 id 发布新版本",
                    project_id
                )));
            }
        } else {
            storage
                .create_project(
                    project_id,
                    &self.manifest.name,
                    &format!(
                        "公司业务包 {} v{}",
                        self.manifest.name, self.manifest.version
                    ),
                    &self.ontology_source,
                )
                .await?;
        }

        let mut report = InstallReport {
            project_id: project_id.clone(),
            objects_created: 0,
            objects_skipped: 0,
            relations_created: 0,
            relations_skipped: 0,
        };
        for object in &self.bootstrap.objects {
            match storage.get(&object.id).await? {
                Some(existing)
                    if existing.r#type == object.object_type && existing.data == object.data =>
                {
                    report.objects_skipped += 1;
                }
                Some(existing) => {
                    return Err(DomainPackError::Invalid(format!(
                        "对象 {} 已存在但内容不同（已有类型 {}）",
                        existing.id, existing.r#type
                    )));
                }
                None => {
                    storage
                        .apply_world(WorldCommand::CreateObject {
                            object_type: object.object_type.clone(),
                            id: object.id.clone(),
                            data: object.data.clone(),
                            project_id: Some(project_id.clone()),
                        })
                        .await?;
                    report.objects_created += 1;
                }
            }
        }
        let existing = storage.list_all_relations().await?;
        for relation in &self.bootstrap.relations {
            if existing.iter().any(|r| {
                r.source == relation.source
                    && r.relation == relation.relation
                    && r.target == relation.target
            }) {
                report.relations_skipped += 1;
                continue;
            }
            storage
                .apply_world(WorldCommand::LinkRelation {
                    source: relation.source.clone(),
                    edge: relation.relation.clone(),
                    target: relation.target.clone(),
                    project_id: Some(project_id.clone()),
                })
                .await?;
            report.relations_created += 1;
        }
        Ok(report)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, DomainPackError> {
    Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
}

fn safe_child(root: &Path, relative: &str) -> Result<PathBuf, DomainPackError> {
    let path = Path::new(relative);
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(DomainPackError::Invalid(format!(
            "业务包文件必须位于包目录内：{relative}"
        )));
    }
    Ok(root.join(path))
}

fn validate_identifier(label: &str, value: &str) -> Result<(), DomainPackError> {
    if value.is_empty()
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(DomainPackError::Invalid(format!(
            "{label} 只能包含字母、数字、-、_、."
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_validates_bootstrap_against_ontology() {
        let dir = std::env::temp_dir().join(format!(
            "oaip_domain_pack_test_{}_{}",
            std::process::id(),
            crate::trace::now_ts()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("manifest.json"),
            r#"{"id":"demo","name":"Demo","version":"1.0.0","ontology":"ontology.odl","bootstrap":"bootstrap.json"}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("ontology.odl"),
            "object Customer:\n  properties:\n    name: string\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("bootstrap.json"),
            r#"{"objects":[{"id":"customer:1","type":"Customer","data":{"name":"A"}}],"relations":[]}"#,
        )
        .unwrap();
        let pack = DomainPack::load(&dir).unwrap();
        assert_eq!(pack.manifest.id, "demo");
        assert_eq!(pack.bootstrap.objects.len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }
}
