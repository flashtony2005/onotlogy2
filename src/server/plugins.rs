//! 通用插件目录 HTTP API。
//!
//! 这里不出现任何彩票专用路由或领域分支；HTTP 只负责目录查询、权限和
//! JSON 传输，真正的能力解析与执行由 `PluginCatalog` 完成。

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde_json::Value;

use crate::auth::AuthUser;
use crate::plugin::catalog::{CatalogError, PluginInfo};

use super::{ApiResponse, AppState, forbidden};

#[derive(Debug, Serialize)]
pub(crate) struct PluginSummary {
    pub id: String,
    pub name: String,
    pub version: String,
    pub status: String,
    pub runtime: String,
    pub endpoint: Option<String>,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PluginManifestView {
    pub status: String,
    pub manifest: crate::plugin::PluginManifest,
}

fn summary(info: PluginInfo) -> PluginSummary {
    PluginSummary {
        id: info.manifest.id,
        name: info.manifest.name,
        version: info.manifest.version,
        status: format!("{:?}", info.status),
        runtime: format!("{:?}", info.manifest.runtime),
        endpoint: info.manifest.endpoint,
        capabilities: info
            .manifest
            .capabilities
            .into_iter()
            .map(|capability| capability.id)
            .collect(),
    }
}

pub(crate) async fn list_plugins(
    State(state): State<Arc<AppState>>,
) -> Json<ApiResponse<Vec<PluginSummary>>> {
    ApiResponse::ok(
        state
            .plugin_catalog
            .list()
            .into_iter()
            .map(summary)
            .collect(),
    )
}

pub(crate) async fn get_plugin_manifest(
    State(state): State<Arc<AppState>>,
    Path(plugin_id): Path<String>,
) -> Response {
    match state.plugin_catalog.manifest(&plugin_id) {
        Some(info) => ApiResponse::ok(PluginManifestView {
            status: format!("{:?}", info.status),
            manifest: info.manifest,
        })
        .into_response(),
        None => catalog_error_response(CatalogError::PluginNotFound(plugin_id)),
    }
}

pub(crate) async fn invoke_plugin_capability(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path((plugin_id, capability)): Path<(String, String)>,
    Json(input): Json<Value>,
) -> Response {
    if !user.can_trigger_run() {
        return forbidden("当前角色无权执行插件能力（需 Operator 及以上）");
    }

    match state
        .plugin_catalog
        .invoke(&plugin_id, &capability, input)
        .await
    {
        Ok(output) => ApiResponse::ok(output).into_response(),
        Err(error) => catalog_error_response(error),
    }
}

fn catalog_error_response(error: CatalogError) -> Response {
    let (status, code) = match &error {
        CatalogError::PluginNotFound(_) | CatalogError::CapabilityNotDeclared { .. } => {
            (axum::http::StatusCode::NOT_FOUND, 404)
        }
        CatalogError::PluginNotActive(_) => (axum::http::StatusCode::CONFLICT, 409),
        CatalogError::Registry(_) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, 5000),
        CatalogError::Runtime(runtime) => match runtime {
            crate::plugin::runtime::RuntimeError::NoContract(_)
            | crate::plugin::runtime::RuntimeError::Unresolved(_) => {
                (axum::http::StatusCode::NOT_FOUND, 404)
            }
            crate::plugin::runtime::RuntimeError::PermissionDenied(_) => {
                (axum::http::StatusCode::FORBIDDEN, 403)
            }
            crate::plugin::runtime::RuntimeError::PluginFailed(_) => {
                (axum::http::StatusCode::INTERNAL_SERVER_ERROR, 5000)
            }
        },
    };
    (status, ApiResponse::<()>::error(code, error.to_string())).into_response()
}
