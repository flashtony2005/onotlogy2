//! RunEnd Hook —— Runtime 热路径上的评测触发器（11A）。
//!
//! 边界（必须守住）：**EvaluationReducer 是纯函数，不关心 HTTP / DB / Agent / UI；
//! Runtime 负责「RunEnd → invoke EvaluationReducer」**。本模块就是 Runtime 侧的编排点：
//!
//! ```text
//! RunEnd → events → EvaluationReducer::evaluate → persist(wb_evaluation)
//!        → emit(EvaluationCreated)
//! ```
//!
//! 同时提供 kernel TraceEvent → SessionEvent 的桥（V1 planner 生产路径词汇表），
//! 使真实 run 的 trace 事实能进入评测事件流。

use serde_json::Value;

use crate::evaluation::{EvaluationPolicy, EvaluationReducer, EvaluationResult, TaskDefinition};
use crate::session::event::{EventType, SessionEvent};
use crate::session::log::SessionLog;
use crate::storage::StorageError;

/// Run 结束钩子：评测（纯函数）→ 持久化 → 发射 EvaluationCreated（append 回事件流）。
///
/// 幂等：同 run 重复调用会覆盖 wb_evaluation（派生表语义），EvaluationCreated 追加新事件。
pub async fn on_run_end(
    log: &mut SessionLog,
    run_id: &str,
    task: &TaskDefinition,
    policy: &EvaluationPolicy,
    storage: &crate::storage::LibSQLStorage,
) -> Result<EvaluationResult, StorageError> {
    let events: Vec<SessionEvent> = log.events().to_vec();
    let result = EvaluationReducer::new(policy.clone()).evaluate(&events, run_id, task);
    storage.save_evaluation(&result).await?;
    // Phase 11C：任务定义持久化（历史任务集 = 按 task_id 聚合的 wb_evaluation + wb_task）。
    storage.save_task(task).await?;
    log.append(
        EventType::EvaluationCreated,
        serde_json::json!({
            "run_id": run_id,
            "evaluation_id": format!("eval.{run_id}"),
            "aggregate": result.aggregate,
            "unknowns": result.unknowns(),
        }),
    );
    Ok(result)
}

/// kernel TraceEvent → (EventType, data) 桥（V1 planner 生产路径词汇表）。
///
/// 只映射对 Evaluation 有意义的 trace 类型；其余（Goal/Intent/Reasoning/Planning/
/// Decision/OntologyQuery/MemoryRecall 等认知步骤）不进入评测事件流（文档化，避免噪声）。
///
/// | trace event_type | SessionEvent | data |
/// |------------------|--------------|------|
/// | `PolicyCheck`    | policy.checked | { capability, verdict: deny/allow/require_approval } |
/// | `Action`         | tool.call     | { capability, ok } |
/// | `ApprovalRequested` | approval.requested | { capability } |
/// | `ApprovalResolved`  | approval.resolved | { capability, decision } |
pub fn trace_to_session_events(trace: &[crate::runtime::TraceEvent]) -> Vec<(EventType, Value)> {
    trace
        .iter()
        .filter_map(|t| {
            let input = t.input.as_ref();
            let output = t.output.as_ref();
            let capability = input
                .and_then(|v| v.get("capability"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            match t.event_type.as_str() {
                "PolicyCheck" => {
                    let verdict = output
                        .and_then(|v| v.as_str())
                        .map(|s| match s {
                            "Deny" => "deny",
                            "ALLOW" => "allow",
                            _ => "require_approval",
                        })
                        .unwrap_or("require_approval");
                    Some((
                        EventType::PolicyChecked,
                        serde_json::json!({ "capability": capability, "verdict": verdict }),
                    ))
                }
                "Action" => {
                    let ok = output
                        .and_then(|v| v.as_str())
                        .map(|s| s == "Success")
                        .unwrap_or(false);
                    Some((
                        EventType::ToolCall,
                        serde_json::json!({ "capability": capability, "ok": ok }),
                    ))
                }
                "ApprovalRequested" => Some((
                    EventType::ApprovalRequested,
                    serde_json::json!({ "capability": capability }),
                )),
                "ApprovalResolved" => {
                    let decision = output
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_lowercase())
                        .unwrap_or_else(|| "approved".to_string());
                    Some((
                        EventType::ApprovalResolved,
                        serde_json::json!({ "capability": capability, "decision": decision }),
                    ))
                }
                _ => None,
            }
        })
        .collect()
}

/// 把桥接结果追加进 SessionLog（保持 trace 顺序；seq 由 log 分配）。
pub fn append_bridged(log: &mut SessionLog, bridged: Vec<(EventType, Value)>) {
    for (event_type, data) in bridged {
        log.append(event_type, data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{TraceEvent, TraceKind};
    use crate::session::event::SessionId;
    use serde_json::json;

    fn trace(event_type: &str, input: Option<Value>, output: Option<Value>) -> TraceEvent {
        TraceEvent {
            id: format!("tr_{}", event_type),
            kind: TraceKind::Cognitive,
            event_type: event_type.to_string(),
            object_refs: vec!["customer_1".into()],
            run_id: Some("run_1".into()),
            agent_id: Some("agent_1".into()),
            reasoning: None,
            input,
            output,
            confidence: None,
            timestamp: "1".into(),
        }
    }

    #[test]
    fn bridge_maps_policy_check_verdicts() {
        let trace = vec![
            trace(
                "PolicyCheck",
                Some(json!({ "capability": "code.test" })),
                Some(json!("ALLOW")),
            ),
            trace(
                "PolicyCheck",
                Some(json!({ "capability": "database.delete" })),
                Some(json!("Deny")),
            ),
            trace("Goal", Some(json!({ "goal": "reduce churn" })), None),
        ];
        let bridged = trace_to_session_events(&trace);
        assert_eq!(bridged.len(), 2, "Goal 不进入评测事件流");
        assert_eq!(bridged[0].0, EventType::PolicyChecked);
        assert_eq!(bridged[0].1["verdict"], json!("allow"));
        assert_eq!(bridged[1].1["verdict"], json!("deny"));
    }

    #[test]
    fn bridge_maps_action_and_approval() {
        let trace = vec![
            trace(
                "Action",
                Some(json!({ "capability": "customer.retain" })),
                Some(json!("Success")),
            ),
            trace(
                "ApprovalResolved",
                Some(json!({ "capability": "customer.refund" })),
                Some(json!("Rejected")),
            ),
        ];
        let bridged = trace_to_session_events(&trace);
        assert_eq!(bridged.len(), 2);
        assert_eq!(bridged[0].0, EventType::ToolCall);
        assert_eq!(bridged[0].1["ok"], json!(true));
        assert_eq!(bridged[1].1["decision"], json!("rejected"));
    }

    #[tokio::test]
    async fn on_run_end_persists_and_emits() {
        let storage = crate::storage::LibSQLStorage::open(":memory:")
            .await
            .unwrap();
        let mut log = SessionLog::new(SessionId::new("run_1"));
        log.append(
            EventType::TestResult,
            json!({ "tests_total": 10, "tests_passed": 9 }),
        );
        let task = TaskDefinition {
            task_id: "t1".into(),
            expected_relations: vec![],
            expected_artifacts: vec![],
            time_budget_secs: 0.0,
            skill_version: None,
        };
        let r = on_run_end(
            &mut log,
            "run_1",
            &task,
            &EvaluationPolicy::default(),
            &storage,
        )
        .await
        .unwrap();
        assert_eq!(r.correctness.value, Some(0.9));

        // wb_evaluation 可查询
        let loaded = storage
            .list_evaluations(Some("run_1"), None, 100)
            .await
            .unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].correctness.value, Some(0.9));

        // EvaluationCreated 已 append 回事件流（事件本身可回放）
        let last = log.events().last().unwrap();
        assert_eq!(last.event_type, EventType::EvaluationCreated);
        assert_eq!(last.data["evaluation_id"], json!("eval.run_1"));
        assert_eq!(last.data["aggregate"], json!(0.9));

        // Replay：同一事件流重算 → 完全一致
        let events: Vec<SessionEvent> = log.events().to_vec();
        let replay = EvaluationReducer::default().evaluate(&events, "run_1", &task);
        assert_eq!(replay.correctness.value, Some(0.9), "回放得到相同评测");
    }
}
