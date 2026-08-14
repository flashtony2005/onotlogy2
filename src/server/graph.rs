use std::sync::Arc;

/// `GET /api/v1/wb/graph`：对象-关系图（V0.8 GraphRAG 基础）。
/// 服务端角色钳制：Executive（最低权限）仅得节点/边骨架，剥除 data 细节
/// （与 timeline/WS 同一套最小权限策略，避免 /graph 成为 V0.7 安全模型的旁路）。
use crate::auth::{AuthUser, Role};
use axum::extract::{Extension, State};
use axum::response::Response;
use serde_json::{Value, json};

use super::*;
pub(crate) async fn list_graph(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
) -> Response {
    let (objects, relations) = match st.kernel.storage().get_object_graph().await {
        Ok(g) => g,
        Err(_) => return ApiResponse::<()>::error(5000, "存储错误").into_response(),
    };
    let detail = user.role.rank() >= Role::Operator.rank();
    let nodes: Vec<Value> = objects
        .into_iter()
        .map(|o| {
            if detail {
                json!({ "id": o.id, "type": o.r#type, "data": o.data })
            } else {
                json!({ "id": o.id, "type": o.r#type })
            }
        })
        .collect();
    let edges: Vec<Value> = relations
        .into_iter()
        .map(|r| {
            if detail {
                json!({
                    "id": r.id,
                    "source": r.source,
                    "target": r.target,
                    "type": r.relation,
                    "data": r.data
                })
            } else {
                json!({
                    "id": r.id,
                    "source": r.source,
                    "target": r.target,
                    "type": r.relation
                })
            }
        })
        .collect();
    ApiResponse::ok(json!({ "nodes": nodes, "edges": edges })).into_response()
}

// ──  handlers  ───────────────────────────────────────────────────────────
