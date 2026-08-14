use std::sync::Arc;

/// `POST /api/v1/wb/agent/run`：触发 Agent 运行（异步，立即返回 runId + streamUrl）。
use crate::auth::AuthUser;
use crate::planner::{
    AutoApprover, Planner, PlannerError, PolicyApprover, RunStatus, RuntimeConfig, ServerApprover,
    generate_run_id, lock_mutex,
};
use axum::Json;
use axum::extract::{Extension, State};
use axum::response::Response;
use serde::Serialize;
use serde_json::Value;

use super::*;
#[derive(serde::Deserialize)]
#[allow(dead_code)]
pub(crate) struct RunBody {
    #[serde(rename = "agentId", default)]
    agent_id: Option<String>,
    goal: String,
    #[serde(rename = "objectRefs", default)]
    object_refs: Option<Vec<String>>,
    #[serde(default)]
    context: Option<Value>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunStartDto {
    run_id: String,
    status: String,
    stream_url: String,
}

pub(crate) async fn run_start(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Json(body): Json<RunBody>,
) -> Response {
    // 触发运行需 Operator 及以上（Executive 只读）。
    if !user.can_trigger_run() {
        return forbidden("当前角色无权触发运行（需 Operator 及以上）");
    }
    let agent_id = match body.agent_id {
        Some(a) if !a.is_empty() => a,
        _ => return ApiResponse::error(wb_code("WB1001"), "agentId 必填").into_response(),
    };
    if body.goal.trim().is_empty() {
        return ApiResponse::error(wb_code("WB1001"), "goal 必填").into_response();
    }
    // 快速健康检查 → WB2004（避免悬挂连接，pi-doc / prd 4.5）
    if !st.engine.health().await {
        return ApiResponse::error(wb_code("WB2004"), "认知引擎不可用（Pi 未就绪）")
            .into_response();
    }

    let run_id = generate_run_id();
    lock_mutex(&st.runs).insert(
        run_id.clone(),
        RunRecord {
            status: RunStatus::Running,
            human_approval: false,
        },
    );
    // 持久化 run（DB 真相源，跨重启可恢复状态）。
    let _ = st
        .kernel
        .storage()
        .insert_run(&run_id, &agent_id, &body.goal)
        .await;

    let kernel = st.kernel.clone();
    let engine = st.engine.clone();
    let st2 = st.clone();
    let store = st.store.clone();
    let auto_approve = st.auto_approve;
    let goal = body.goal.clone();
    let ag = agent_id.clone();
    let rid = run_id.clone();
    tokio::spawn(async move {
        // Human Approval 闸门：
        // - auto_approve → 直接放行（AutoApprover）；
        // - 否则 `require_approval` 动作挂起，注册待审批请求并等待浏览器经 HTTP 端点裁决
        //   （ServerApprover），批准则恢复执行，拒绝则 WB2005 终止。
        let approver: Arc<dyn PolicyApprover> = if auto_approve {
            Arc::new(AutoApprover { allow: true })
        } else {
            let mut sa = ServerApprover::new(kernel.clone(), store, rid.clone(), ag.clone());
            if let Some(t) = st.approval_timeout {
                sa = sa.with_timeout(t);
            }
            Arc::new(sa)
        };
        let res = Planner::with_actions(&kernel, st.registry.clone())
            .with_approver(approver)
            .with_config(RuntimeConfig {
                auto_approve_from_evolution: st.auto_approve_evolution,
            })
            .run_goal(engine.as_ref(), &goal, &ag, &rid)
            .await;
        let rec = match res {
            Ok(_) => RunRecord {
                status: RunStatus::Completed,
                human_approval: false,
            },
            Err(PlannerError::PolicyDenied(_)) => RunRecord {
                status: RunStatus::Failed,
                human_approval: true,
            },
            Err(_) => RunRecord {
                status: RunStatus::Failed,
                human_approval: false,
            },
        };
        // 持久化终态（与内存映射并行）。
        let _ = st2
            .kernel
            .storage()
            .update_run_status(&rid, rec.status.as_str(), Some(&now_micros()))
            .await;
        lock_mutex(&st2.runs).insert(rid, rec);
    });

    let dto = RunStartDto {
        run_id: run_id.clone(),
        status: "Running".into(),
        stream_url: format!("/api/v1/wb/agent/{run_id}/stream"),
    };
    ApiResponse::ok(dto).into_response()
}

/// WebSocket 握手：`/api/v1/wb/agent/:id/stream`。
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_start_dto_is_camel_case() {
        let dto = RunStartDto {
            run_id: "run_1".into(),
            status: "Running".into(),
            stream_url: "/api/v1/wb/agent/run_1/stream".into(),
        };
        let json = serde_json::to_value(dto).unwrap();
        assert_eq!(json["runId"], "run_1");
        assert_eq!(json["streamUrl"], "/api/v1/wb/agent/run_1/stream");
        assert!(json.get("run_id").is_none());
        assert!(json.get("stream_url").is_none());
    }
}
