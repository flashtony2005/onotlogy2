//! 业务包目录查询与安装 API（Phase 2.3）。
//!
//! 业务包内容仍由 `DomainPack::load` 统一校验；HTTP 层只负责目录发现、权限、
//! 状态汇总和安装结果传输，不复制本体/Bootstrap 解析逻辑。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::auth::AuthUser;
use crate::domain_pack::{DomainPack, DomainPackError};
use crate::storage::{ObjectEngine, WorldCommand};

use super::{ApiResponse, AppState, forbidden};

#[derive(Debug, Serialize)]
pub(crate) struct DomainPackSummary {
    pub id: String,
    pub name: String,
    pub version: String,
    pub minimum_runtime: Option<String>,
    pub skill_ids: Vec<String>,
    pub status: String,
    pub valid: bool,
    pub installed: bool,
    pub ontology_types: usize,
    pub bootstrap_objects: usize,
    pub bootstrap_relations: usize,
    pub enabled: bool,
    pub plugin_ids: Vec<String>,
    pub plugin_capabilities: Vec<String>,
    pub error: Option<String>,
}

pub(crate) use crate::domain_pack::DomainPackRuntime;

#[derive(Debug, Deserialize)]
pub(crate) struct DomainPackRuntimeUpdate {
    pub enabled: bool,
    #[serde(default)]
    pub source_path: String,
    #[serde(default)]
    pub watch_enabled: bool,
    #[serde(default = "default_poll_interval")]
    pub poll_interval_seconds: u64,
    #[serde(default)]
    pub auto_sync: bool,
    #[serde(default)]
    pub auto_analyze: bool,
    #[serde(default)]
    pub auto_backtest: bool,
    #[serde(default)]
    pub auto_review: bool,
    #[serde(default)]
    pub strategy_ids: Vec<String>,
}

fn default_poll_interval() -> u64 {
    30
}

#[derive(Debug, Serialize)]
pub(crate) struct DomainPackPluginSummary {
    pub id: String,
    pub name: String,
    pub status: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct StrategyRegistryEntry {
    pub id: String,
    pub code: String,
    pub name: String,
    pub family: String,
    pub mode: String,
    pub description: String,
    pub enabled: bool,
    pub version: String,
    pub parameters: Value,
    pub training_window: Option<usize>,
    pub backtest_count: usize,
    pub mean_red_hits: Option<f64>,
    pub mean_blue_hits: Option<f64>,
    pub random_baseline_red_hits: Option<f64>,
    pub random_baseline_blue_hits: Option<f64>,
    pub publishable: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct DomainPackOperations {
    pub pack_id: String,
    pub runtime: DomainPackRuntime,
    pub plugins: Vec<DomainPackPluginSummary>,
    pub strategies: Vec<StrategyRegistryEntry>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DomainPackInstallResult {
    pub pack: DomainPackSummary,
    pub project_id: String,
    pub objects_created: usize,
    pub objects_skipped: usize,
    pub relations_created: usize,
    pub relations_skipped: usize,
}

fn runtime_object_id(pack_id: &str) -> String {
    format!("domain-pack-runtime:{pack_id}")
}

fn default_runtime(pack_id: &str) -> DomainPackRuntime {
    DomainPackRuntime {
        pack_id: pack_id.to_string(),
        enabled: true,
        source_path: String::new(),
        watch_enabled: false,
        poll_interval_seconds: 30,
        auto_sync: false,
        auto_analyze: false,
        auto_backtest: false,
        auto_review: false,
        strategy_ids: Vec::new(),
        updated_by: String::new(),
        updated_at: String::new(),
    }
}

async fn runtime_for_pack(
    state: &AppState,
    pack_id: &str,
) -> Result<DomainPackRuntime, crate::storage::StorageError> {
    let Some(record) = state.storage.get(&runtime_object_id(pack_id)).await? else {
        return Ok(default_runtime(pack_id));
    };
    serde_json::from_value(record.data).map_err(crate::storage::StorageError::from)
}

fn runtime_from_update(
    pack_id: &str,
    user: &AuthUser,
    update: DomainPackRuntimeUpdate,
) -> Result<DomainPackRuntime, String> {
    if !(1..=86_400).contains(&update.poll_interval_seconds) {
        return Err("poll_interval_seconds 必须在 1 到 86400 之间".into());
    }
    Ok(DomainPackRuntime {
        pack_id: pack_id.to_string(),
        enabled: update.enabled,
        source_path: update.source_path,
        watch_enabled: update.watch_enabled,
        poll_interval_seconds: update.poll_interval_seconds,
        auto_sync: update.auto_sync,
        auto_analyze: update.auto_analyze,
        auto_backtest: update.auto_backtest,
        auto_review: update.auto_review,
        strategy_ids: update.strategy_ids,
        updated_by: user.username.clone(),
        updated_at: oaip_trace::now_ts(),
    })
}

fn value_string(data: &Value, key: &str) -> String {
    data.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn value_bool(data: &Value, key: &str) -> bool {
    data.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn value_f64(data: &Value, key: &str) -> Option<f64> {
    data.get(key).and_then(Value::as_f64)
}

fn strategy_matches(strategy_id: &str, code: &str, reference: &str) -> bool {
    reference == strategy_id || reference == code || reference.ends_with(&format!(":{code}"))
}

fn associated_plugins(
    state: &AppState,
    pack: &DomainPack,
) -> (Vec<DomainPackPluginSummary>, Vec<String>) {
    let capabilities: std::collections::BTreeSet<String> = {
        let registry = state
            .v2
            .skills
            .read()
            .expect("Skill Registry RwLock 不中毒");
        pack.manifest
            .skills
            .iter()
            .filter_map(|id| registry.get(id))
            .flat_map(|skill| skill.capabilities.iter().cloned())
            .collect()
    };
    let mut plugins = Vec::new();
    for info in state.plugin_catalog.list() {
        let plugin_capabilities: Vec<String> = info
            .manifest
            .capabilities
            .iter()
            .map(|capability| capability.id.clone())
            .filter(|capability| capabilities.contains(capability))
            .collect();
        if !plugin_capabilities.is_empty() {
            plugins.push(DomainPackPluginSummary {
                id: info.manifest.id,
                name: info.manifest.name,
                status: format!("{:?}", info.status),
                capabilities: plugin_capabilities,
            });
        }
    }
    let matched_capabilities = capabilities.into_iter().collect();
    (plugins, matched_capabilities)
}

async fn strategy_registry(
    state: &AppState,
    pack_id: &str,
) -> Result<Vec<StrategyRegistryEntry>, crate::storage::StorageError> {
    let definitions = state
        .storage
        .list_objects_for_project(pack_id, "AnalysisStrategy")
        .await?;
    let runs = state
        .storage
        .list_objects_for_project(pack_id, "AnalysisRun")
        .await?;
    let mut entries = Vec::new();
    for definition in definitions {
        let data = &definition.data;
        let code = value_string(data, "code");
        let mut backtest_count = 0;
        let mut latest_metrics: Option<Value> = None;
        for run in &runs {
            let reference = value_string(&run.data, "strategy");
            if !strategy_matches(&definition.id, &code, &reference) {
                continue;
            }
            let status = value_string(&run.data, "status");
            let summary = run
                .data
                .get("result_summary")
                .and_then(Value::as_str)
                .and_then(|raw| serde_json::from_str::<Value>(raw).ok());
            if status == "backtested" {
                backtest_count += 1;
                if summary.is_some() {
                    latest_metrics = summary;
                }
            }
        }
        let metrics = latest_metrics.as_ref();
        let mean_red_hits = metrics.and_then(|v| value_f64(v, "mean_red_hits"));
        let mean_blue_hits = metrics.and_then(|v| value_f64(v, "mean_blue_hits"));
        let random_red = metrics.and_then(|v| value_f64(v, "random_baseline_red_hits"));
        let random_blue = metrics.and_then(|v| value_f64(v, "random_baseline_blue_hits"));
        let enabled = value_bool(data, "enabled");
        let publishable = enabled
            && backtest_count > 0
            && mean_red_hits
                .zip(random_red)
                .is_some_and(|(actual, baseline)| actual >= baseline)
            && mean_blue_hits
                .zip(random_blue)
                .is_some_and(|(actual, baseline)| actual >= baseline);
        let version = {
            let value = value_string(data, "version");
            if value.is_empty() {
                "1.0.0".to_string()
            } else {
                value
            }
        };
        entries.push(StrategyRegistryEntry {
            id: definition.id,
            code,
            name: value_string(data, "name"),
            family: value_string(data, "family"),
            mode: value_string(data, "mode"),
            description: value_string(data, "description"),
            enabled,
            version,
            parameters: data.get("parameters").cloned().unwrap_or_else(|| json!({})),
            training_window: data
                .get("training_window")
                .and_then(Value::as_u64)
                .map(|v| v as usize),
            backtest_count,
            mean_red_hits,
            mean_blue_hits,
            random_baseline_red_hits: random_red,
            random_baseline_blue_hits: random_blue,
            publishable,
        });
    }
    entries.sort_by(|a, b| a.code.cmp(&b.code));
    Ok(entries)
}

fn pack_directories(root: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut dirs = Vec::new();
    if !root.is_dir() {
        return Ok(dirs);
    }
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            dirs.push(entry.path());
        }
    }
    dirs.sort();
    Ok(dirs)
}

fn invalid_summary(path: &Path, error: &DomainPackError) -> DomainPackSummary {
    let id = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown")
        .to_string();
    DomainPackSummary {
        id,
        name: String::new(),
        version: String::new(),
        minimum_runtime: None,
        skill_ids: Vec::new(),
        status: "invalid".into(),
        valid: false,
        installed: false,
        ontology_types: 0,
        bootstrap_objects: 0,
        bootstrap_relations: 0,
        enabled: false,
        plugin_ids: Vec::new(),
        plugin_capabilities: Vec::new(),
        error: Some(error.to_string()),
    }
}

async fn summary(
    state: &AppState,
    pack: &DomainPack,
) -> Result<DomainPackSummary, crate::storage::StorageError> {
    let installed = state.storage.project(&pack.manifest.id).await?.is_some();
    let runtime = runtime_for_pack(state, &pack.manifest.id).await?;
    let (plugins, plugin_capabilities) = associated_plugins(state, pack);
    let plugin_ids = plugins.iter().map(|plugin| plugin.id.clone()).collect();
    Ok(DomainPackSummary {
        id: pack.manifest.id.clone(),
        name: pack.manifest.name.clone(),
        version: pack.manifest.version.clone(),
        minimum_runtime: pack.manifest.minimum_runtime.clone(),
        skill_ids: pack.manifest.skills.clone(),
        status: if !installed {
            "available"
        } else if runtime.enabled {
            "installed"
        } else {
            "disabled"
        }
        .into(),
        valid: true,
        installed,
        ontology_types: pack.ontology.object_types.len(),
        bootstrap_objects: pack.bootstrap.objects.len(),
        bootstrap_relations: pack.bootstrap.relations.len(),
        enabled: installed && runtime.enabled,
        plugin_ids,
        plugin_capabilities,
        error: None,
    })
}

#[allow(clippy::result_large_err)]
async fn load_pack_or_404(state: &AppState, id: &str) -> Result<DomainPack, Response> {
    match find_pack(&state.domain_packs_dir, id) {
        Ok(Some(pack)) => Ok(pack),
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            ApiResponse::error(4040, format!("未找到有效业务包：{id}")),
        )
            .into_response()),
        Err(error) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiResponse::error(5000, format!("业务包目录读取失败：{error}")),
        )
            .into_response()),
    }
}

#[allow(clippy::result_large_err)]
async fn require_installed(state: &AppState, id: &str) -> Result<DomainPack, Response> {
    let pack = load_pack_or_404(state, id).await?;
    match state.storage.project(id).await {
        Ok(Some(_)) => Ok(pack),
        Ok(None) => Err((
            StatusCode::CONFLICT,
            ApiResponse::error(4090, "业务包尚未安装，不能配置运行能力"),
        )
            .into_response()),
        Err(error) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiResponse::error(5000, format!("业务包安装状态读取失败：{error}")),
        )
            .into_response()),
    }
}

async fn save_runtime(
    state: &AppState,
    runtime: &DomainPackRuntime,
) -> Result<(), crate::storage::StorageError> {
    let id = runtime_object_id(&runtime.pack_id);
    let data = serde_json::to_value(runtime)?;
    if state.storage.get(&id).await?.is_some() {
        state
            .storage
            .apply_world(WorldCommand::UpdateObject {
                id,
                data,
                project_id: Some(runtime.pack_id.clone()),
            })
            .await
    } else {
        state
            .storage
            .apply_world(WorldCommand::CreateObject {
                object_type: "DomainPackRuntime".into(),
                id,
                data,
                project_id: Some(runtime.pack_id.clone()),
            })
            .await
    }
}

pub(crate) async fn get_domain_pack_runtime(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if !user.can_read_audit() {
        return forbidden("当前角色无权读取业务包运行配置（需 Operator 及以上）");
    }
    if let Err(response) = load_pack_or_404(&state, &id).await {
        return response;
    }
    match runtime_for_pack(&state, &id).await {
        Ok(runtime) => ApiResponse::ok(runtime).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiResponse::error(5000, format!("运行配置读取失败：{error}")),
        )
            .into_response(),
    }
}

pub(crate) async fn update_domain_pack_runtime(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    AxumPath(id): AxumPath<String>,
    Json(update): Json<DomainPackRuntimeUpdate>,
) -> Response {
    if !user.can_manage_domain_packs() {
        return forbidden("当前角色无权修改业务包运行配置（仅 Developer）");
    }
    if let Err(response) = require_installed(&state, &id).await {
        return response;
    }
    let runtime = match runtime_from_update(&id, &user, update) {
        Ok(runtime) => runtime,
        Err(error) => {
            return (StatusCode::BAD_REQUEST, ApiResponse::error(4000, error)).into_response();
        }
    };
    match save_runtime(&state, &runtime).await {
        Ok(()) => ApiResponse::ok(runtime).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiResponse::error(5000, format!("运行配置保存失败：{error}")),
        )
            .into_response(),
    }
}

async fn set_domain_pack_enabled(
    state: Arc<AppState>,
    user: AuthUser,
    id: String,
    enabled: bool,
) -> Response {
    if !user.can_manage_domain_packs() {
        return forbidden("当前角色无权启停业务包（仅 Developer）");
    }
    if let Err(response) = require_installed(&state, &id).await {
        return response;
    }
    let mut runtime = match runtime_for_pack(&state, &id).await {
        Ok(runtime) => runtime,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiResponse::error(5000, format!("运行配置读取失败：{error}")),
            )
                .into_response();
        }
    };
    runtime.enabled = enabled;
    runtime.updated_by = user.username;
    runtime.updated_at = oaip_trace::now_ts();
    match save_runtime(&state, &runtime).await {
        Ok(()) => ApiResponse::ok(runtime).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiResponse::error(5000, format!("业务包状态保存失败：{error}")),
        )
            .into_response(),
    }
}

pub(crate) async fn enable_domain_pack(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    set_domain_pack_enabled(state, user, id, true).await
}

pub(crate) async fn disable_domain_pack(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    set_domain_pack_enabled(state, user, id, false).await
}

pub(crate) async fn get_domain_pack_operations(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if !user.can_read_audit() {
        return forbidden("当前角色无权读取业务包运行能力（需 Operator 及以上）");
    }
    let pack = match load_pack_or_404(&state, &id).await {
        Ok(pack) => pack,
        Err(response) => return response,
    };
    let runtime = match runtime_for_pack(&state, &id).await {
        Ok(runtime) => runtime,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiResponse::error(5000, format!("运行配置读取失败：{error}")),
            )
                .into_response();
        }
    };
    let (plugins, _) = associated_plugins(&state, &pack);
    let strategies = match strategy_registry(&state, &id).await {
        Ok(strategies) => strategies,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiResponse::error(5000, format!("策略注册表读取失败：{error}")),
            )
                .into_response();
        }
    };
    ApiResponse::ok(DomainPackOperations {
        pack_id: id,
        runtime,
        plugins,
        strategies,
    })
    .into_response()
}

pub(crate) async fn get_domain_pack_strategies(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if !user.can_read_audit() {
        return forbidden("当前角色无权读取策略注册表（需 Operator 及以上）");
    }
    if let Err(response) = load_pack_or_404(&state, &id).await {
        return response;
    }
    match strategy_registry(&state, &id).await {
        Ok(strategies) => ApiResponse::ok(strategies).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiResponse::error(5000, format!("策略注册表读取失败：{error}")),
        )
            .into_response(),
    }
}

pub(crate) async fn list_domain_packs(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
) -> Response {
    if !user.can_read_audit() {
        return forbidden("当前角色无权读取业务包（需 Operator 及以上）");
    }

    let dirs = match pack_directories(&state.domain_packs_dir) {
        Ok(dirs) => dirs,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiResponse::error(5000, format!("业务包目录读取失败：{error}")),
            )
                .into_response();
        }
    };

    let mut packs = Vec::with_capacity(dirs.len());
    for dir in dirs {
        match DomainPack::load(&dir) {
            Ok(pack) => match summary(&state, &pack).await {
                Ok(item) => packs.push(item),
                Err(error) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        ApiResponse::error(5000, format!("业务包状态读取失败：{error}")),
                    )
                        .into_response();
                }
            },
            Err(error) => packs.push(invalid_summary(&dir, &error)),
        }
    }

    packs.sort_by(|a, b| a.id.cmp(&b.id));
    ApiResponse::ok(packs).into_response()
}

fn find_pack(root: &Path, id: &str) -> Result<Option<DomainPack>, std::io::Error> {
    for dir in pack_directories(root)? {
        match DomainPack::load(&dir) {
            Ok(pack) if pack.manifest.id == id => return Ok(Some(pack)),
            Ok(_) | Err(_) => {}
        }
    }
    Ok(None)
}

pub(crate) async fn install_domain_pack(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if !user.can_manage_domain_packs() {
        return forbidden("当前角色无权安装业务包（仅 Developer）");
    }

    let pack = match find_pack(&state.domain_packs_dir, &id) {
        Ok(Some(pack)) => pack,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                ApiResponse::error(4040, format!("未找到有效业务包：{id}")),
            )
                .into_response();
        }
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiResponse::error(5000, format!("业务包目录读取失败：{error}")),
            )
                .into_response();
        }
    };

    let report = match pack.install(&state.storage).await {
        Ok(report) => report,
        Err(error) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                ApiResponse::error(4220, format!("业务包安装失败：{error}")),
            )
                .into_response();
        }
    };
    let pack_summary = match summary(&state, &pack).await {
        Ok(summary) => summary,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiResponse::error(5000, format!("业务包状态读取失败：{error}")),
            )
                .into_response();
        }
    };

    ApiResponse::ok(DomainPackInstallResult {
        pack: pack_summary,
        project_id: report.project_id,
        objects_created: report.objects_created,
        objects_skipped: report.objects_skipped,
        relations_created: report.relations_created,
        relations_skipped: report.relations_skipped,
    })
    .into_response()
}
