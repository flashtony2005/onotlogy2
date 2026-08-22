//! M1b · G1 契约测试：V2 AgentLoop 认知 Trace 事件流（v2_parity）。
//!
//! 契约（与 V1 run_goal 同源，UI buildTimeline/deriveState 依赖）：
//! - 事件 kind=Cognitive，绑定 run_id / agent_id；object_refs 非空（空世界兜底 "world"）。
//! - 序列：Goal 开头 → OntologyQuery → Reasoning → Intent → Planning →
//!   PolicyCheck → Action → MemoryUpdate 结尾（Completed 时）。
//! - 审批：Waiting 时 PolicyCheck=RequireApproval + PendingApproval，无 Action/MemoryUpdate；
//!   显式审批裁决续跑（同一 sink）后 Action + MemoryUpdate，且 Goal 不重复。
//! - 拒绝：PolicyCheck=Deny，无 Action、无 MemoryUpdate（deriveState → Failed）。
//! - 默认不开 trace（with_trace 是 opt-in，sink 不被触碰）。

use openaip_os::cognitive::engine::{AgentView, FakeEngine, PlanStep};
use openaip_os::cognitive::{Agent, AgentLoop, LoopOutcome};
use openaip_os::compose::policy::{Cmp, Policy, Precondition};
use openaip_os::compose::{Capability, Service, Task};
use openaip_os::kernel::effect::Reversibility;
use openaip_os::kernel::{Kernel, Scope};
use openaip_os::trace::{TraceEvent, TraceKind};
use serde_json::json;

fn crm_service() -> Service {
    Service::new("crm-service")
        .with_capability(
            Capability::new("customer.activate", "ActivateCustomer")
                .with_reversibility(Reversibility::Reversible)
                .with_projection("status", "active")
                .requiring(&["target"]),
        )
        .with_capability(
            Capability::new("customer.refund", "RefundCustomer")
                .with_reversibility(Reversibility::Compensatable)
                .with_inverse("UndoRefund")
                .with_policy(Policy {
                    require_approval: true,
                    preconditions: vec![Precondition {
                        param: "amount".into(),
                        op: Cmp::Le,
                        value: json!(1000),
                    }],
                })
                .with_projection("refunded", "true")
                .requiring(&["target", "amount"]),
        )
}

fn spawn_agent(k: &mut Kernel) -> Agent {
    let world = k.create_context(None, Scope::World, "world").unwrap();
    Agent::new("retention-agent", world)
}

fn task_fiber_of(k: &Kernel, agent: &Agent, task: &Task) -> openaip_os::kernel::FiberId {
    k.all_fibers()
        .find(|f| f.context == agent.context && f.task == task.id)
        .map(|f| f.id)
        .expect("task fiber 应存在")
}

/// 事件序列的类型名（按发生顺序）。
fn kinds(trace: &[TraceEvent]) -> Vec<&str> {
    trace.iter().map(|e| e.event_type.as_str()).collect()
}

fn event<'a>(trace: &'a [TraceEvent], ty: &str) -> &'a TraceEvent {
    trace
        .iter()
        .find(|e| e.event_type == ty)
        .unwrap_or_else(|| panic!("缺少事件 {ty}: {:?}", kinds(trace)))
}

/// 全事件绑定契约（kind/run/agent/refs）。
fn assert_binding(trace: &[TraceEvent], run_id: &str, agent_id: &str) {
    for e in trace {
        assert_eq!(
            e.kind,
            TraceKind::Cognitive,
            "{} 应为认知事件",
            e.event_type
        );
        assert_eq!(
            e.run_id.as_deref(),
            Some(run_id),
            "{} run 绑定",
            e.event_type
        );
        assert_eq!(
            e.agent_id.as_deref(),
            Some(agent_id),
            "{} agent 绑定",
            e.event_type
        );
        assert!(
            !e.object_refs.is_empty(),
            "{} 应绑定对象（空世界兜底 world）",
            e.event_type
        );
    }
}

// ── G1a：闭环完整流水线 ────────────────────────────────────────────────
#[test]
fn g1a_closed_loop_emits_full_pipeline() {
    let mut k = Kernel::new();
    let mut agent = spawn_agent(&mut k);
    let services = [crm_service()];
    let engine = FakeEngine::new(vec![PlanStep {
        capability: "customer.activate".into(),
        args: json!({ "target": "u1" }),
        when: Some(Box::new(|v: &AgentView| {
            !v.has_relation("status", "active")
        })),
    }]);
    let task = Task::new("activate-u1", "激活用户 u1").expecting(&[("status", "active")]);
    let ctx = agent.context;

    let mut sink: Vec<TraceEvent> = Vec::new();
    let out = {
        let mut lp =
            AgentLoop::new(&mut k, &mut agent, &services, &engine).with_trace("run_t1", &mut sink);
        lp.run_until_done(ctx, &task, json!({})).unwrap()
    };
    assert_eq!(out, LoopOutcome::Completed);

    let types = kinds(&sink);
    assert_eq!(types.first(), Some(&"Goal"), "Goal 开头: {types:?}");
    assert_eq!(
        types.last(),
        Some(&"MemoryUpdate"),
        "MemoryUpdate 结尾: {types:?}"
    );
    for needed in [
        "OntologyQuery",
        "Reasoning",
        "Intent",
        "Planning",
        "PolicyCheck",
        "Action",
    ] {
        assert!(types.contains(&needed), "缺少事件 {needed}: {types:?}");
    }
    assert_binding(&sink, "run_t1", "retention-agent");
    assert_eq!(
        event(&sink, "PolicyCheck").output,
        Some(json!("Allow")),
        "无审批能力 → Allow"
    );
    // 决策事件带置信度（UI risk 计算源）
    let reasoning = event(&sink, "Reasoning");
    assert!(reasoning.confidence.is_some(), "Reasoning 应带 confidence");
}

// ── G1b：审批挂起 → PendingApproval → 续跑完成，Goal 不重复 ────────────
#[test]
fn g1b_waiting_emits_pending_approval_then_resume_completes() {
    let mut k = Kernel::new();
    let mut agent = spawn_agent(&mut k);
    let services = [crm_service()];
    let engine = FakeEngine::new(vec![PlanStep {
        capability: "customer.refund".into(),
        args: json!({ "target": "u1", "amount": 500 }),
        when: Some(Box::new(|v: &AgentView| {
            !v.has_relation("refunded", "true")
        })),
    }]);
    let task = Task::new("refund", "退款 500").expecting(&[("refunded", "true")]);
    let ctx = agent.context;

    let mut sink: Vec<TraceEvent> = Vec::new();
    let out = {
        let mut lp =
            AgentLoop::new(&mut k, &mut agent, &services, &engine).with_trace("run_t2", &mut sink);
        lp.run_until_done(ctx, &task, json!({})).unwrap()
    };
    assert_eq!(out, LoopOutcome::Waiting, "require_approval 应挂起");
    let fiber = task_fiber_of(&k, &agent, &task);
    {
        let types = kinds(&sink);
        assert_eq!(
            event(&sink, "PolicyCheck").output,
            Some(json!("RequireApproval"))
        );
        assert!(
            types.contains(&"PendingApproval"),
            "应有 PendingApproval: {types:?}"
        );
        assert!(!types.contains(&"Action"), "挂起不应有 Action: {types:?}");
        assert!(
            !types.contains(&"MemoryUpdate"),
            "挂起不应有 MemoryUpdate: {types:?}"
        );
    }

    // 人工审批 → 续跑（同一 sink：Goal 不重复、事件追加）
    k.approve(fiber, "admin").unwrap();
    let goals_before = sink.iter().filter(|e| e.event_type == "Goal").count();
    let out2 = {
        let mut lp =
            AgentLoop::new(&mut k, &mut agent, &services, &engine).with_trace("run_t2", &mut sink);
        lp.resume_until_done(fiber, &task).unwrap()
    };
    assert_eq!(out2, LoopOutcome::Completed);
    let types = kinds(&sink);
    assert!(types.contains(&"Action"), "续跑后应有 Action: {types:?}");
    assert_eq!(
        types.last(),
        Some(&"MemoryUpdate"),
        "续跑完成后 MemoryUpdate 结尾: {types:?}"
    );
    assert_eq!(
        sink.iter().filter(|e| e.event_type == "Goal").count(),
        goals_before,
        "resume 不得重复 Goal"
    );
    assert_binding(&sink, "run_t2", "retention-agent");
}

// ── G1c：前置条件拒绝 → PolicyCheck Deny，无 Action/MemoryUpdate ──────
#[test]
fn g1c_deny_emits_policy_deny_no_action_no_memory() {
    let mut k = Kernel::new();
    let mut agent = spawn_agent(&mut k);
    let services = [crm_service()];
    let engine = FakeEngine::new(vec![PlanStep {
        capability: "customer.refund".into(),
        args: json!({ "target": "u1", "amount": 5000 }),
        when: Some(Box::new(|v: &AgentView| {
            !v.has_relation("refunded", "true")
        })),
    }]);
    let task = Task::new("refund", "退款 5000（超限）").expecting(&[("refunded", "true")]);
    let ctx = agent.context;

    let mut sink: Vec<TraceEvent> = Vec::new();
    let out = {
        let mut lp =
            AgentLoop::new(&mut k, &mut agent, &services, &engine).with_trace("run_t3", &mut sink);
        lp.run_until_done(ctx, &task, json!({})).unwrap()
    };
    assert_eq!(
        out,
        LoopOutcome::Denied("前置条件不满足: amount <= Number(1000)（实际 Number(5000)）".into())
    );

    let types = kinds(&sink);
    let pc = event(&sink, "PolicyCheck");
    assert_eq!(pc.output, Some(json!("Deny")), "超限应 Deny");
    assert_eq!(pc.confidence, Some(0.0), "Deny 零置信（与 V1 同源）");
    assert!(!types.contains(&"Action"), "拒绝不应有 Action: {types:?}");
    assert!(
        !types.contains(&"MemoryUpdate"),
        "拒绝不应有 MemoryUpdate（deriveState → Failed）: {types:?}"
    );
    assert_binding(&sink, "run_t3", "retention-agent");
}

// ── G1d：默认不开 trace（opt-in，sink 不被触碰）─────────────────────────
#[test]
fn g1d_trace_off_by_default() {
    let mut k = Kernel::new();
    let mut agent = spawn_agent(&mut k);
    let services = [crm_service()];
    let engine = FakeEngine::new(vec![PlanStep {
        capability: "customer.activate".into(),
        args: json!({ "target": "u1" }),
        when: Some(Box::new(|v: &AgentView| {
            !v.has_relation("status", "active")
        })),
    }]);
    let task = Task::new("activate-u1", "激活用户 u1").expecting(&[("status", "active")]);
    let ctx = agent.context;

    let sink: Vec<TraceEvent> = Vec::new();
    let out = {
        let mut lp = AgentLoop::new(&mut k, &mut agent, &services, &engine);
        lp.run_until_done(ctx, &task, json!({})).unwrap()
    };
    assert_eq!(out, LoopOutcome::Completed);
    assert!(sink.is_empty(), "未开 trace 不得写 sink");
}
