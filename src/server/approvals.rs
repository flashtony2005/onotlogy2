//! 挂起审批端点（M1c · V2）：`GET /run/:run_id/approvals` + `POST /run/:run_id/approval/:req_id`。
//!
//! V1 读 ApprovalStore（planner 内存 + wb_approval 持久化）；M1c 起读 `AppState.pending`
//! 注册表（run 路径 Waiting 时注册，裁决经 mpsc 通道唤醒 run 的 resolver）。
//! 已裁决条目从注册表移除 → 二次裁决 404（V1 tc023 语义）。

use std::sync::Arc;

use crate::auth::AuthUser;
use crate::cognitive::approval::ApprovalDecision;
use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::response::Response;
use serde::Serialize;
use serde_json::json;

use super::*;

/// 挂起审批列表项（HTTP 契约：reqId/runId/capability/action/args/state）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PendingApprovalDto {
    req_id: String,
    run_id: String,
    capability: String,
    action: String,
    args: serde_json::Value,
    state: String,
}

pub(crate) async fn list_approvals(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(run_id): Path<String>,
) -> Response {
    // 读审批列表需 Operator 及以上。
    if !user.can_read_audit() {
        return forbidden("当前角色无权读取审批（需 Operator 及以上）");
    }
    let g = lock_mutex(&st.pending);
    let mut list: Vec<PendingApprovalDto> = g
        .values()
        .filter(|p| p.run_id == run_id)
        .map(|p| PendingApprovalDto {
            req_id: p.request.id.clone(),
            run_id: p.run_id.clone(),
            capability: p.request.capability.clone(),
            action: p.request.action.clone(),
            args: p.request.args.clone(),
            state: "Pending".to_string(),
        })
        .collect();
    list.sort_by(|a, b| a.req_id.cmp(&b.req_id));
    ApiResponse::ok(list).into_response()
}

#[derive(serde::Deserialize)]
pub(crate) struct ApprovalDecisionBody {
    /// `approve` | `yes` | `true` → 批准；其余 → 拒绝（WB2005 终止）。
    decision: String,
}

/// `POST /api/v1/wb/agent/run/:run_id/approval/:req_id`：裁决一条待审批请求，唤醒挂起的运行。
pub(crate) async fn resolve_approval(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path((run_id, req_id)): Path<(String, String)>,
    Json(body): Json<ApprovalDecisionBody>,
) -> Response {
    // 裁决审批需 Operator 及以上。
    if !user.can_resolve_approval() {
        return forbidden("当前角色无权裁决审批（需 Operator 及以上）");
    }
    let entry = lock_mutex(&st.pending).remove(&req_id);
    let Some(p) = entry else {
        return ApiResponse::error(404, "审批请求不存在或已裁决").into_response();
    };
    if p.run_id != run_id {
        // 错 run 的请求放回注册表（请求仍待裁决）
        lock_mutex(&st.pending).insert(req_id, p);
        return ApiResponse::error(404, "审批请求不属于该运行").into_response();
    }
    let approve = matches!(
        body.decision.to_ascii_lowercase().as_str(),
        "approve" | "yes" | "true"
    );
    let decision = if approve {
        ApprovalDecision::Approve {
            by: user.id.clone(),
        }
    } else {
        ApprovalDecision::Reject {
            by: user.id.clone(),
        }
    };
    // T1：fabric 挂起——裁决时同步续跑（不经 tx/resolver；fabric submit 无阻塞等待方）
    if let super::ResumeTarget::FabricRun { session_id, run_id } = &p.resume {
        let session_id = session_id.clone();
        let run_id = run_id.clone();
        let out =
            tokio::task::block_in_place(|| st.facade.fabric_resolve(&session_id, &run_id, approve));
        return match out {
            Ok(msg) => ApiResponse::ok(json!({
                "reqId": req_id,
                "decision": if approve { "Approved" } else { "Rejected" },
                "status": msg.payload["status"],
                "sender": msg.sender,
            }))
            .into_response(),
            Err(e) => {
                // 拒绝本身是预期（fabric_resolve 对 reject 返回 Denied 错误）
                if !approve {
                    return ApiResponse::ok(json!({
                        "reqId": req_id,
                        "decision": "Rejected",
                        "status": "rejected",
                    }))
                    .into_response();
                }
                ApiResponse::error(5000, format!("fabric 续跑失败: {e}")).into_response()
            }
        };
    }
    if p.tx.send(decision).is_err() {
        return ApiResponse::error(404, "审批请求已终止").into_response();
    }
    ApiResponse::ok(json!({
        "reqId": req_id,
        "decision": if approve { "Approved" } else { "Rejected" }
    }))
    .into_response()
}
