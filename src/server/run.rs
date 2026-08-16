//! `POST /api/v1/wb/agent/run`：触发 Agent 运行（M1c · V2 认知闭环）。
//!
//! V1 走 Planner::run_goal；M1c 起走 V2：`spawn_blocking` 内跑 `cognitive::run_agent`
//! （同步闭环 + 可能的 pi 子进程），挂起审批经 mpsc 通道与 HTTP 端点对接——
//! resolver 阻塞等待裁决，HTTP 端点经 `AppState.pending` 注册表 send 决策。
//! 终局后：trace 落库 wb_trace_event、run 状态写 wb_run、Evaluation 桥接（I7）。

use std::sync::Arc;
use std::time::Instant;

use crate::auth::AuthUser;
use crate::cognitive::approval::{ApprovalDecision, ApprovalRequest};
use crate::cognitive::runner::run_agent;
use crate::cognitive::{Agent, LoopOutcome};
use crate::evaluation::hook::{append_bridged, on_run_end, trace_to_session_events};
use crate::evaluation::{EvaluationPolicy, TaskDefinition};
use crate::evolution2::EvolutionEngine;
use crate::kernel::{Kernel, Scope};
use crate::planner::{RunStatus, lock_mutex};
use crate::session::event::{EventType, SessionId};
use crate::session::log::SessionLog;
use crate::session::run::generate_run_id;
use crate::storage::TraceQuery;
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
    // 快速健康检查 → WB2004（V2：CognitiveEngine::health，PiEngine 探测二进制）
    if !st.v2_engine.health() {
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
    // 持久化 run（DB 真相源，跨重启可恢复状态；G7：wb_run = V2 run store）。
    let _ = st
        .kernel
        .storage()
        .insert_run(&run_id, &agent_id, &body.goal)
        .await;

    // run 上下文（blocking 线程内使用；全部 Send + 'static）
    let st2 = st.clone();
    let goal = body.goal.clone();
    let rid = run_id.clone();
    let agent = agent_id.clone();
    let auto_approve = st.auto_approve;
    let services = Arc::clone(&st.v2_services);
    let engine = Arc::clone(&st.v2_engine);
    let pending = Arc::clone(&st.pending);
    let approval_timeout = st.approval_timeout;

    // run 在 blocking 线程执行（同步闭环 + 可能的 pi 子进程）；JoinHandle 有意不等待
    // （HTTP 端点经 pending 注册表与 run 交互），故显式 drop。
    let run_task = tokio::task::spawn_blocking(move || {
        let handle = tokio::runtime::Handle::current();
        let mut kernel = Kernel::new();
        let mut agent_state = match kernel.create_context(None, Scope::World, "world") {
            Ok(ctx) => Agent::new(agent.clone(), ctx),
            Err(e) => {
                handle.block_on(async {
                    let _ = st2
                        .kernel
                        .storage()
                        .update_run_status(&rid, "Failed", Some(&crate::runtime::now_ts()))
                        .await;
                });
                lock_mutex(&st2.runs).insert(
                    rid.clone(),
                    RunRecord {
                        status: RunStatus::Failed,
                        human_approval: false,
                    },
                );
                eprintln!("run {rid}: 创建 Context 失败: {e}");
                return;
            }
        };
        let mut trace: Vec<crate::runtime::TraceEvent> = Vec::new();
        let mut approvals: Vec<ApprovalRequest> = Vec::new();

        // 审批 resolver：注册到 pending（HTTP 端点可见）→ 阻塞等待裁决。
        // 超时看门狗：TTL 到期仍挂起 → 经通道发 Expire（audit 与 Reject 区分）。
        let pending2 = Arc::clone(&pending);
        let rid2 = rid.clone();
        let timeout2 = approval_timeout;
        let mut resolver = move |req: &ApprovalRequest| -> ApprovalDecision {
            if auto_approve {
                return ApprovalDecision::Approve {
                    by: "server-auto".into(),
                };
            }
            let (tx, rx) = std::sync::mpsc::channel();
            {
                let mut g = lock_mutex(&pending2);
                g.insert(
                    req.id.clone(),
                    PendingApproval {
                        run_id: rid2.clone(),
                        request: req.clone(),
                        tx: tx.clone(),
                        created_at: Instant::now(),
                    },
                );
            }
            // 看门狗：超时未裁决 → Expire（若条目仍在注册表）
            let h = tokio::runtime::Handle::current();
            let req_id = req.id.clone();
            let tx2 = tx.clone();
            let pending3 = Arc::clone(&pending2);
            let ttl = timeout2.unwrap_or(crate::planner::APPROVAL_TIMEOUT);
            h.spawn(async move {
                tokio::time::sleep(ttl).await;
                if lock_mutex(&pending3).remove(&req_id).is_some() {
                    let _ = tx2.send(ApprovalDecision::Expire {
                        by: "timeout".into(),
                    });
                }
            });
            // 阻塞等待裁决（spawn_blocking 线程内，不占 tokio worker）
            rx.recv().unwrap_or(ApprovalDecision::Expire {
                by: "channel-closed".into(),
            })
        };

        // 演化引擎：auto_approve_evolution → Gate 放行路径（M2 统一持久化）
        let mut evolution = if st2.auto_approve_evolution {
            Some(EvolutionEngine::new().with_gate_enabled(true))
        } else {
            None
        };

        let report = match run_agent(
            &mut kernel,
            &mut agent_state,
            services.as_slice(),
            engine.as_ref(),
            Some(st2.v2_effect_defs.as_ref().clone()),
            evolution.as_mut(),
            &rid,
            &goal,
            &mut trace,
            &mut approvals,
            &mut resolver,
        ) {
            Ok(r) => r,
            Err(e) => {
                handle.block_on(async {
                    let _ = st2
                        .kernel
                        .storage()
                        .update_run_status(&rid, "Failed", Some(&crate::runtime::now_ts()))
                        .await;
                });
                lock_mutex(&st2.runs).insert(
                    rid.clone(),
                    RunRecord {
                        status: RunStatus::Failed,
                        human_approval: false,
                    },
                );
                eprintln!("run {rid}: {e}");
                return;
            }
        };

        // 终局落库：trace → wb_trace_event；run 状态 → wb_run；Evaluation 桥接。
        let rid3 = rid.clone();
        let goal3 = goal.clone();
        let agent3 = agent.clone();
        let st3 = st2.clone();
        handle.block_on(async move {
            for e in &trace {
                let _ = st3.kernel.storage().append_trace(e).await;
            }
            let _ = st3
                .kernel
                .storage()
                .update_run_status(
                    &rid3,
                    report.status.as_str(),
                    Some(&crate::runtime::now_ts()),
                )
                .await;

            let mut log = SessionLog::new(SessionId::new(&rid3));
            log.append(
                EventType::ContextAssembled,
                serde_json::json!({
                    "run_id": rid3,
                    "assets": [
                        { "asset_id": "goal", "selection_reason": goal3, "relevance": 1.0 },
                        { "asset_id": "agent", "selection_reason": agent3, "relevance": 1.0 },
                    ],
                }),
            );
            if let Ok(stored) = st3.kernel.list_events(TraceQuery::for_run(&rid3)).await {
                append_bridged(&mut log, trace_to_session_events(&stored));
            }
            log.append(
                EventType::SessionCompleted,
                serde_json::json!({ "run_id": rid3 }),
            );
            let task = TaskDefinition {
                task_id: goal3,
                expected_relations: vec![],
                expected_artifacts: vec![],
                time_budget_secs: 0.0,
                skill_version: None,
            };
            let _ = on_run_end(
                &mut log,
                &rid3,
                &task,
                &EvaluationPolicy::default(),
                &st3.kernel.storage(),
            )
            .await;
        });

        let rec = RunRecord {
            status: report.status,
            human_approval: matches!(report.outcome, LoopOutcome::Denied(_))
                || report.human_approval,
        };
        lock_mutex(&st2.runs).insert(rid, rec);
    });
    drop(run_task);

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
