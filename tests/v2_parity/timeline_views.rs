//! M1b · G9 契约测试：三层时间线在 V2 事件流下行为不变
//! （v0_3 tc009 三层视图 / v0_9_ws_clamp 的 V2 等价——UI buildTimeline 契约）。
//!
//! 用 V2 AgentLoop 的 trace sink 产出真实事件流 → trace::build_timeline / derive_run_state
//! 断言：状态推导（Completed/Required）、进度 8 步、Executive 只含 Goal+Action、
//! Operator 8 步骨架、Developer 全量。UI 不用改一行即可消费 V2 事件。

use openaip_os::cognitive::engine::{AgentView, FakeEngine, PlanStep};
use openaip_os::cognitive::{Agent, AgentLoop, LoopOutcome};
use openaip_os::compose::policy::Policy;
use openaip_os::compose::{Capability, Service, Task};
use openaip_os::kernel::effect::Reversibility;
use openaip_os::kernel::relation::Relation;
use openaip_os::kernel::{Kernel, Scope};
use openaip_os::session::run::RunStatus;
use openaip_os::trace::TraceEvent;
use openaip_os::trace::{View, build_timeline, derive_run_state};
use serde_json::json;

fn approval_service() -> Service {
    Service::new("svc").with_capability(
        Capability::new("customer.refund", "RefundCustomer")
            .with_reversibility(Reversibility::Compensatable)
            .with_policy(Policy {
                require_approval: true,
                preconditions: vec![],
            })
            .with_projection("refunded", "true")
            .requiring(&["target", "amount"]),
    )
}

fn spawn_agent(k: &mut Kernel) -> Agent {
    let world = k.create_context(None, Scope::World, "world").unwrap();
    Agent::new("agent", world)
}

/// 完整审批闭环（挂起 → relate 批准 → 续跑完成），收集 V2 事件流。
fn completed_run_with_approval() -> Vec<TraceEvent> {
    let mut k = Kernel::new();
    let mut agent = spawn_agent(&mut k);
    let services = [approval_service()];
    let engine = FakeEngine::new(vec![PlanStep {
        capability: "customer.refund".into(),
        args: json!({ "target": "u1", "amount": 100 }),
        when: Some(Box::new(|v: &AgentView| {
            !v.has_relation("refunded", "true")
        })),
    }]);
    let task = Task::new("refund", "退款").expecting(&[("refunded", "true")]);
    let ctx = agent.context;

    let mut sink: Vec<TraceEvent> = Vec::new();
    let out = {
        let mut lp =
            AgentLoop::new(&mut k, &mut agent, &services, &engine).with_trace("run_tl", &mut sink);
        lp.run_until_done(ctx, &task, json!({})).unwrap()
    };
    assert_eq!(out, LoopOutcome::Waiting);
    let fiber = k
        .all_fibers()
        .find(|f| f.context == ctx && f.task == task.id)
        .unwrap()
        .id;
    k.relate(Relation::new(ctx, "admin", "Approval", "x", k.now()))
        .unwrap();
    let out2 = {
        let mut lp =
            AgentLoop::new(&mut k, &mut agent, &services, &engine).with_trace("run_tl", &mut sink);
        lp.resume_until_done(fiber, &task).unwrap()
    };
    assert_eq!(out2, LoopOutcome::Completed);
    sink
}

// ── G9b：derive_run_state（UI 状态推导契约）───────────────────────────
#[test]
fn g9b_v2_events_drive_state_derivation() {
    let events = completed_run_with_approval();
    let (status, progress, human) = derive_run_state(&events);
    assert_eq!(status, RunStatus::Completed, "MemoryUpdate → Completed");
    assert_eq!(progress, 100, "8 步全齐 → 100%");
    assert_eq!(human, "Required", "有 PendingApproval → Required");
}

// ── G9c：三层视图裁剪（Executive/Operator/Developer）──────────────────
#[test]
fn g9c_v2_events_drive_three_view_clamping() {
    let events = completed_run_with_approval();

    let exec = build_timeline(
        events.clone(),
        "run_tl".to_string(),
        "agent".to_string(),
        View::Executive,
    );
    assert_eq!(exec.status, "Completed");
    assert!(exec.decision.is_some(), "Executive 应有决策摘要");
    assert!(
        exec.events
            .iter()
            .all(|e| e.event_type == "Goal" || e.event_type == "Action"),
        "Executive 只含 Goal/Action: {:?}",
        exec.events
            .iter()
            .map(|e| e.event_type.as_str())
            .collect::<Vec<_>>()
    );

    let op = build_timeline(
        events.clone(),
        "run_tl".to_string(),
        "agent".to_string(),
        View::Operator,
    );
    assert_eq!(op.events.len(), 8, "Operator = 8 步骨架（每步首现）");
    assert_eq!(op.progress, 100);

    let dev = build_timeline(
        events.clone(),
        "run_tl".to_string(),
        "agent".to_string(),
        View::Developer,
    );
    assert!(dev.events.len() > 8, "Developer 全量事件");
    // Developer 全量含 PendingApproval（8 步之外的事件也可见）
    assert!(dev.events.iter().any(|e| e.event_type == "PendingApproval"));
}
