//! 彩票能力插件：把彩票业务能力接入统一 PluginRuntime。
//!
//! 当前版本复用已经验证过的 `lottery` 领域实现，统一暴露能力入口；
//! 后续再把数据源、WorldCommand 提交和自动化 Job 进一步拆开。

use crate::lottery::{self, LotteryError, LotteryGame};
use crate::plugin::capability::{CapabilityContract, CapabilityRegistry, Router, RouterStrategy};
use crate::plugin::manifest::{Permissions, PluginManifest, RuntimeType};
use crate::plugin::registry::{PluginRegistry, RegistryError};
use crate::plugin::runtime::{PluginRuntime, RuntimeError};
use crate::plugin::{AsyncPlugin, PluginError};
use crate::storage::LibSQLStorage;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use thiserror::Error;

pub const PLUGIN_ID: &str = "lottery-analysis";
pub const IMPORT_CAPABILITY: &str = "lottery.import";
pub const SYNC_CAPABILITY: &str = "lottery.data.sync";
pub const ANALYZE_CAPABILITY: &str = "lottery.analyze";
pub const BACKTEST_CAPABILITY: &str = "lottery.backtest";
pub const REVIEW_CAPABILITY: &str = "lottery.review";

/// 彩票能力插件的统一执行入口。
///
/// 同一入口可被 CLI、HTTP、网页和自动化 Job 复用；调用方不再直接依赖
/// `lottery::import_csv/analyze/backtest/run_with_pi` 的内部函数。
pub struct LotteryPlugin {
    manifest: PluginManifest,
    storage: LibSQLStorage,
    source_roots: Arc<RwLock<Vec<PathBuf>>>,
}

impl LotteryPlugin {
    pub fn new(storage: LibSQLStorage) -> Self {
        let roots = std::env::var_os("ONTOLOGY_LOTTERY_SOURCE_ROOT")
            .map(|value| std::env::split_paths(&value).collect())
            .unwrap_or_else(|| vec![std::env::temp_dir()]);
        Self::with_source_roots(storage, roots)
    }

    pub fn with_source_roots(
        storage: LibSQLStorage,
        source_roots: impl IntoIterator<Item = PathBuf>,
    ) -> Self {
        let source_roots = source_roots
            .into_iter()
            .filter_map(|root| root.canonicalize().ok())
            .filter(|root| root.is_dir())
            .collect::<Vec<_>>();
        let filesystem_read = source_roots
            .iter()
            .map(|root| root.to_string_lossy().into_owned())
            .collect();
        let manifest =
            PluginManifest::new(PLUGIN_ID, "彩票本体论分析", "1.0.0", RuntimeType::Local)
                .with_capability(IMPORT_CAPABILITY)
                .with_capability(SYNC_CAPABILITY)
                .with_capability(ANALYZE_CAPABILITY)
                .with_capability(BACKTEST_CAPABILITY)
                .with_capability(REVIEW_CAPABILITY)
                .with_permissions(Permissions {
                    filesystem_read,
                    database: vec!["ontology.read".into(), "ontology.command".into()],
                    ..Permissions::default()
                });
        Self {
            manifest,
            storage,
            source_roots: Arc::new(RwLock::new(source_roots)),
        }
    }

    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    pub fn source_roots(&self) -> Arc<RwLock<Vec<PathBuf>>> {
        Arc::clone(&self.source_roots)
    }

    pub fn allow_source_root(&self, root: &Path) -> Result<(), PluginError> {
        let canonical = root
            .canonicalize()
            .map_err(|error| PluginError::Failed(format!("数据源目录不可用：{error}")))?;
        if !canonical.is_dir() {
            return Err(PluginError::Failed(format!(
                "数据源授权路径不是目录：{}",
                canonical.display()
            )));
        }
        let mut roots = self
            .source_roots
            .write()
            .map_err(|_| PluginError::Failed("数据源授权状态不可用".into()))?;
        if !roots.iter().any(|root| root == &canonical) {
            roots.push(canonical);
        }
        Ok(())
    }

    fn authorize_source_file(&self, file: &str) -> Result<PathBuf, PluginError> {
        let canonical = PluginRuntime::canonicalize_filesystem_read(Path::new(file))
            .map_err(|error| PluginError::Failed(error.to_string()))?;
        let roots = self
            .source_roots
            .read()
            .map_err(|_| PluginError::Failed("数据源授权状态不可用".into()))?;
        if roots.iter().any(|root| canonical.starts_with(root)) {
            Ok(canonical)
        } else {
            Err(PluginError::Failed(format!(
                "权限拒绝: filesystem_read {}",
                canonical.display()
            )))
        }
    }
}

#[async_trait::async_trait]
impl AsyncPlugin for LotteryPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    async fn execute_async(&self, capability: &str, input: Value) -> Result<Value, PluginError> {
        let result = match capability {
            IMPORT_CAPABILITY | SYNC_CAPABILITY => {
                let game = parse_game(&input)?;
                let file = required_string(&input, "file")?;
                let file = self.authorize_source_file(file)?;
                let project = required_string(&input, "project")?;
                serde_json::to_value(
                    lottery::import_csv(
                        &self.storage,
                        game,
                        file.to_string_lossy().as_ref(),
                        project,
                    )
                    .await
                    .map_err(to_plugin_error)?,
                )
                .map_err(|e| PluginError::Failed(format!("同步结果序列化失败：{e}")))?
            }
            ANALYZE_CAPABILITY => {
                let game = parse_game(&input)?;
                let window = positive_usize(&input, "window", 50)?;
                let project = required_string(&input, "project")?;
                serde_json::to_value(
                    lottery::analyze(&self.storage, game, window, project)
                        .await
                        .map_err(to_plugin_error)?,
                )
                .map_err(|e| PluginError::Failed(format!("统计结果序列化失败：{e}")))?
            }
            BACKTEST_CAPABILITY => {
                let game = parse_game(&input)?;
                let train_window = positive_usize(&input, "train_window", 50)?;
                let test_count = positive_usize(&input, "test_count", 30)?;
                let project = required_string(&input, "project")?;
                serde_json::to_value(
                    lottery::backtest(&self.storage, game, train_window, test_count, project)
                        .await
                        .map_err(to_plugin_error)?,
                )
                .map_err(|e| PluginError::Failed(format!("回测结果序列化失败：{e}")))?
            }
            REVIEW_CAPABILITY => {
                let game = parse_game(&input)?;
                let window = positive_usize(&input, "window", 50)?;
                let test_count = positive_usize(&input, "test_count", 30)?;
                let project = required_string(&input, "project")?;
                lottery::run_with_pi(&self.storage, game, window, test_count, project)
                    .await
                    .map_err(to_plugin_error)?
            }
            other => {
                return Err(PluginError::Failed(format!(
                    "插件 {} 未声明能力 {other}",
                    self.manifest.id
                )));
            }
        };
        Ok(result)
    }
}

/// 声明彩票插件的输入/输出契约。
///
/// 当前 `CapabilityContract` 保存 JSON 形状示例；后续可替换为正式 JSON Schema。
pub fn declare_capabilities(registry: &mut CapabilityRegistry) {
    registry.declare(CapabilityContract {
        capability: IMPORT_CAPABILITY.into(),
        input_schema: json!({
            "type": "object",
            "required": ["game", "file", "project"],
            "properties": { "game": { "type": "string" }, "file": { "type": "string" }, "project": { "type": "string" } }
        }),
        output_schema: json!({ "type": "object", "properties": { "imported": { "type": "integer" } } }),
        effects: vec!["create_or_update_world_objects".into(), "create_data_import".into()],
    });
    registry.declare(CapabilityContract {
        capability: SYNC_CAPABILITY.into(),
        input_schema: json!({
            "type": "object",
            "required": ["game", "file", "project"],
            "properties": { "game": { "type": "string" }, "file": { "type": "string" }, "project": { "type": "string" } }
        }),
        output_schema: json!({ "type": "object", "properties": { "source_hash": { "type": "string" }, "conflicts": { "type": "integer" } } }),
        effects: vec!["create_or_update_world_objects".into(), "create_data_import_batch".into()],
    });
    registry.declare(CapabilityContract {
        capability: ANALYZE_CAPABILITY.into(),
        input_schema: json!({
            "type": "object",
            "required": ["game", "project"],
            "properties": { "game": { "type": "string" }, "window": { "type": "integer" }, "project": { "type": "string" } }
        }),
        output_schema: json!({ "type": "object", "properties": { "sample_size": { "type": "integer" } } }),
        effects: vec!["create_analysis_run".into()],
    });
    registry.declare(CapabilityContract {
        capability: BACKTEST_CAPABILITY.into(),
        input_schema: json!({
            "type": "object",
            "required": ["game", "project"],
            "properties": { "game": { "type": "string" }, "train_window": { "type": "integer" }, "test_count": { "type": "integer" }, "project": { "type": "string" } }
        }),
        output_schema: json!({ "type": "object", "properties": { "target_count": { "type": "integer" } } }),
        effects: vec!["create_backtest_run".into()],
    });
    registry.declare(CapabilityContract {
        capability: REVIEW_CAPABILITY.into(),
        input_schema: json!({
            "type": "object",
            "required": ["game", "project"],
            "properties": { "game": { "type": "string" }, "window": { "type": "integer" }, "test_count": { "type": "integer" }, "project": { "type": "string" } }
        }),
        output_schema: json!({ "type": "object" }),
        effects: vec!["create_agent_observation".into(), "create_analysis_run".into()],
    });
}

#[derive(Debug, Error)]
pub enum LotteryCapabilityError {
    #[error("彩票插件注册失败：{0}")]
    Registry(#[from] RegistryError),
    #[error("彩票能力执行失败：{0}")]
    Runtime(#[from] RuntimeError),
}

/// 通过统一 PluginRuntime 执行彩票能力。
pub async fn invoke(
    storage: &LibSQLStorage,
    capability: &str,
    input: Value,
) -> Result<Value, LotteryCapabilityError> {
    let plugin = LotteryPlugin::new(storage.clone());
    let manifest = plugin.manifest().clone();

    let mut plugins = PluginRegistry::new();
    plugins.install_async(Box::new(plugin))?;
    plugins.validate(PLUGIN_ID)?;
    plugins.register(PLUGIN_ID)?;
    plugins.activate(PLUGIN_ID)?;

    let mut capabilities = CapabilityRegistry::new();
    declare_capabilities(&mut capabilities);
    capabilities.bind(&manifest);

    let mut runtime = PluginRuntime::new(
        &plugins,
        &capabilities,
        Router::new(RouterStrategy::Explicit(PLUGIN_ID.into())),
    );
    runtime
        .execute_async(capability, input)
        .await
        .map_err(LotteryCapabilityError::from)
}

fn parse_game(input: &Value) -> Result<LotteryGame, PluginError> {
    let value = required_string(input, "game")?;
    LotteryGame::parse(value).map_err(to_plugin_error)
}

fn required_string<'a>(input: &'a Value, name: &str) -> Result<&'a str, PluginError> {
    input
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| PluginError::Failed(format!("缺少必填字段：{name}")))
}

fn positive_usize(input: &Value, name: &str, default: usize) -> Result<usize, PluginError> {
    match input.get(name) {
        None => Ok(default),
        Some(value) => value
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .filter(|value| *value > 0)
            .ok_or_else(|| PluginError::Failed(format!("字段 {name} 必须是正整数"))),
    }
}

fn to_plugin_error(error: LotteryError) -> PluginError {
    PluginError::Failed(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[tokio::test]
    async fn source_file_permission_is_fail_closed() {
        let storage = LibSQLStorage::open(":memory:").await.expect("db");
        storage
            .create_project("lottery-analysis", "彩票", "测试", "ontology")
            .await
            .expect("project");

        let error = invoke(
            &storage,
            IMPORT_CAPABILITY,
            json!({
                "game": "dlt",
                "file": "/etc/passwd",
                "project": "lottery-analysis"
            }),
        )
        .await
        .expect_err("插件不得读取配置数据目录之外的文件");
        assert!(error.to_string().contains("权限拒绝"));
    }

    #[tokio::test]
    async fn source_file_must_be_a_regular_absolute_file() {
        let storage = LibSQLStorage::open(":memory:").await.expect("db");
        let plugin = LotteryPlugin::with_source_roots(
            storage,
            [std::env::temp_dir().join(format!("lottery_plugin_root_{}", std::process::id()))],
        );
        let error = plugin
            .authorize_source_file("relative.csv")
            .expect_err("相对路径必须拒绝");
        assert!(error.to_string().contains("绝对路径"));
    }

    #[tokio::test]
    async fn invoke_round_trip_uses_capability_runtime() {
        let storage = LibSQLStorage::open(":memory:").await.expect("db");
        storage
            .create_project("lottery-analysis", "彩票", "测试", "ontology")
            .await
            .expect("project");
        let path = std::env::temp_dir().join(format!("lottery_plugin_{}.csv", std::process::id()));
        fs::write(
            &path,
            "期号,前区1,前区2,前区3,前区4,前区5,后区1,后区2,日期\n1,01,02,03,04,05,01,02,2026-01-01\n2,01,02,06,07,08,01,03,2026-01-03\n3,02,04,06,08,10,02,04,2026-01-05\n",
        )
        .expect("csv");

        let imported = invoke(
            &storage,
            IMPORT_CAPABILITY,
            json!({ "game": "dlt", "file": path, "project": "lottery-analysis" }),
        )
        .await
        .expect("import");
        assert_eq!(imported["imported"], 3);

        let synced = invoke(
            &storage,
            SYNC_CAPABILITY,
            json!({ "game": "dlt", "file": path, "project": "lottery-analysis" }),
        )
        .await
        .expect("sync");
        assert_eq!(synced["imported"], 0);
        assert_eq!(synced["skipped_existing"], 3);
        assert_eq!(synced["conflicts"], 0);

        let analyzed = invoke(
            &storage,
            ANALYZE_CAPABILITY,
            json!({ "game": "dlt", "window": 2, "project": "lottery-analysis" }),
        )
        .await
        .expect("analyze");
        assert_eq!(analyzed["sample_size"], 2);

        let backtested = invoke(
            &storage,
            BACKTEST_CAPABILITY,
            json!({ "game": "dlt", "train_window": 2, "test_count": 1, "project": "lottery-analysis" }),
        )
        .await
        .expect("backtest");
        assert_eq!(backtested["target_count"], 1);

        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn manifest_declares_lottery_capabilities() {
        let storage = LibSQLStorage::open(":memory:").await.expect("db");
        let manifest = LotteryPlugin::new(storage).manifest().clone();
        assert!(manifest.declares(IMPORT_CAPABILITY));
        assert!(manifest.declares(SYNC_CAPABILITY));
        assert!(manifest.declares(ANALYZE_CAPABILITY));
        assert!(manifest.declares(BACKTEST_CAPABILITY));
        assert!(manifest.declares(REVIEW_CAPABILITY));
    }
}
