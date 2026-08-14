use std::collections::HashMap;

/// `GET /api/v1/wb/agent/run/:run_id/approvals`：列出该运行的全部审批请求（含历史裁决）。
use std::sync::Arc;

use crate::auth::AuthUser;
use crate::planner::{ApprovalRequest, ApprovalState};
use crate::storage::ApprovalRecord;
use axum::extract::{Extension, Path, State};
use axum::response::Response;
use serde_json::json;

use super::*;
pub(crate) async fn list_approvals(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(run_id): Path<String>,
) -> Response {
    // 读审批列表需 Operator 及以上。
    if !user.can_read_audit() {
        return forbidden("当前角色无权读取审批（需 Operator 及以上）");
    }
    // 合并：DB 持久记录（跨重启）+ 内存 store（live 唤醒状态），按 req_id 去重，内存优先。
    let mut by_id: HashMap<String, ApprovalRequest> = HashMap::new();
    if let Ok(rows) = st.kernel.storage().list_approvals_for_run(&run_id).await {
        for r in rows {
            by_id.insert(r.id.clone(), approval_record_to_request(&r));
        }
    }
    for req in st.store.list_for_run(&run_id) {
        by_id.insert(req.req_id.clone(), req);
    }
    let mut list: Vec<ApprovalRequest> = by_id.into_values().collect();
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
    let Some(req) = st.store.get(&req_id) else {
        return ApiResponse::error(404, "审批请求不存在或已裁决").into_response();
    };
    if req.run_id != run_id {
        return ApiResponse::error(404, "审批请求不属于该运行").into_response();
    }
    let approve = matches!(
        body.decision.to_ascii_lowercase().as_str(),
        "approve" | "yes" | "true"
    );
    if st.store.resolve(&req_id, approve) {
        // 持久化裁决（DB 真相源；resolved_by 取当前操作人 id）。
        let decision = if approve { "Approved" } else { "Rejected" };
        let _ = st
            .kernel
            .storage()
            .update_approval_state(&req_id, decision, Some(&user.id), Some(&now_micros()))
            .await;
        ApiResponse::ok(json!({
            "reqId": req_id,
            "decision": if approve { "Approved" } else { "Rejected" }
        }))
        .into_response()
    } else {
        ApiResponse::error(404, "审批请求不存在或已裁决").into_response()
    }
}

/// 将持久审批记录映射为 API `ApprovalRequest`（state 由字符串还原）。
fn approval_record_to_request(r: &ApprovalRecord) -> ApprovalRequest {
    ApprovalRequest {
        req_id: r.id.clone(),
        run_id: r.run_id.clone(),
        agent_id: r.agent_id.clone(),
        step_id: r.step_id.clone(),
        description: r.description.clone(),
        state: ApprovalState::from_db_str(&r.state),
    }
}
