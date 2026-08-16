//! M1c · Runner 契约测试：V2 认知闭环的入口编排（CLI/server 共用）。
//!
//! 契约：
//! - 无审批能力闭环 → Completed + trace 事件流 + Memory 沉淀 + 世界关系达成，resolver 不调用。
//! - require_approval → Waiting 挂起注册 → resolver 裁决 Approve → 续跑 Completed；
//!   resolver 恰好调用一次且请求类型化（capability/action/args）。
//! - resolver Reject → Failed + human_approval=true，世界无变化。
//! - 未知能力决策 → Failed（无人工）。
//! - EvolutionGate 放行（已 Validated + 无失败）→ 不经过 resolver 直接 Completed。

use openaip_os::cognitive::approval::{ApprovalDecision, ApprovalRequest};
use openaip_os::cognitive::engine::{AgentView, FakeEngine, PlanStep};
use openaip_os::cognitive::runner::{RunReport, run_agent};
use openaip_os::cognitive::{Agent, LoopOutcome};
use openaip_os::compose::policy::Policy;
use openaip_os::compose::{Capability, Service};
use openaip_os::evolution2::{EvolutionEngine, OutcomeKind};
use openaip_os::kernel::effect::Reversibility;
use openaip_os::kernel::{Kernel, Scope};
use openaip_os::runtime::TraceEvent;
use openaip_os::session::run::RunStatus;
use serde_json::json;

fn services() -> Vec<Service> {
    vec![
        Service::new("crm").with_capability(
            Capability::new("customer.activate", "ActivateCustomer")
                .with_reversibility(Reversibility::Reversible)
                .with_projection("status", "active")
                .requiring(&["target"]),
        ),
        Service::new("crm").with_capability(
            Capability::new("customer.refund", "RefundCustomer")
                .with_reversibility(Reversibility::Compensatable)
                .with_policy(Policy {
                    require_approval: true,
                    preconditions: vec![],
                })
                .with_projection("refunded", "true")
                .requiring(&["target", "amount"]),
        ),
    ]
}

fn spawn_agent(k: &mut Kernel) -> Agent {
    let world = k.create_context(None, Scope::World, "world").unwrap();
    Agent::new("agent", world)
}

fn approve_all() -> impl FnMut(&ApprovalRequest) -> ApprovalDecision {
    |_req| ApprovalDecision::Approve {
        by: "tester".into(),
    }
}

fn reject_all() -> impl FnMut(&ApprovalRequest) -> ApprovalDecision {
    |_req| ApprovalDecision::Reject {
        by: "tester".into(),
    }
}

// ── R1：无审批闭环（resolver 不调用）──────────────────────────────────
#[test]
fn r1_plain_loop_completes_without_resolver() {
    let mut k = Kernel::new();
    let mut agent = spawn_agent(&mut k);
    let engine = FakeEngine::new(vec![PlanStep {
        capability: "customer.activate".into(),
        args: json!({ "target": "u1" }),
        when: Some(Box::new(|v: &AgentView| {
            !v.has_relation("status", "active")
        })),
    }]);
    let mut trace: Vec<TraceEvent> = Vec::new();
    let mut approvals: Vec<ApprovalRequest> = Vec::new();
    let mut resolver_calls = 0;

    let report: RunReport = run_agent(
        &mut k,
        &mut agent,
        &services(),
        &engine,
        None,
        None,
        "run_r1",
        "激活 u1",
        &mut trace,
        &mut approvals,
        &mut |_req| {
            resolver_calls += 1;
            ApprovalDecision::Approve { by: "x".into() }
        },
    )
    .unwrap();

    assert_eq!(report.outcome, LoopOutcome::Completed);
    assert_eq!(report.status, RunStatus::Completed);
    assert!(!report.human_approval);
    assert_eq!(resolver_calls, 0, "无审批能力不得惊动 resolver");
    assert!(!trace.is_empty(), "trace 事件流非空");
    assert!(trace.iter().any(|e| e.event_type == "MemoryUpdate"));
    assert!(
        k.relations()
            .any(|r| r.predicate == "status" && r.target == "active")
    );
    assert_eq!(agent.memory.hits_of("ActivateCustomer"), 1);
}

// ── R2：挂起 → resolver Approve → 续跑完成（恰好一次裁决）────────────
#[test]
fn r2_approval_via_resolver_completes() {
    let mut k = Kernel::new();
    let mut agent = spawn_agent(&mut k);
    let engine = FakeEngine::new(vec![PlanStep {
        capability: "customer.refund".into(),
        args: json!({ "target": "u1", "amount": 100 }),
        when: Some(Box::new(|v: &AgentView| {
            !v.has_relation("refunded", "true")
        })),
    }]);
    let mut trace: Vec<TraceEvent> = Vec::new();
    let mut approvals: Vec<ApprovalRequest> = Vec::new();
    let mut seen: Vec<(String, String)> = Vec::new();

    let report = run_agent(
        &mut k,
        &mut agent,
        &services(),
        &engine,
        None,
        None,
        "run_r2",
        "退款 u1",
        &mut trace,
        &mut approvals,
        &mut |req| {
            seen.push((req.capability.clone(), req.action.clone()));
            ApprovalDecision::Approve { by: "ops".into() }
        },
    )
    .unwrap();

    assert_eq!(report.outcome, LoopOutcome::Completed);
    assert_eq!(report.status, RunStatus::Completed);
    assert_eq!(
        seen,
        vec![("customer.refund".to_string(), "RefundCustomer".to_string())]
    );
    assert!(
        k.relations()
            .any(|r| r.predicate == "refunded" && r.target == "true")
    );
    // 事件流含挂起 + 执行 + 收尾
    let types: Vec<&str> = trace.iter().map(|e| e.event_type.as_str()).collect();
    assert!(types.contains(&"PendingApproval"));
    assert!(types.contains(&"Action"));
    assert_eq!(types.last(), Some(&"MemoryUpdate"));
}

// ── R3：resolver Reject → Failed + human_approval，世界无变化 ─────────
#[test]
fn r3_reject_fails_with_human_flag() {
    let mut k = Kernel::new();
    let mut agent = spawn_agent(&mut k);
    let engine = FakeEngine::new(vec![PlanStep {
        capability: "customer.refund".into(),
        args: json!({ "target": "u1", "amount": 100 }),
        when: Some(Box::new(|v: &AgentView| {
            !v.has_relation("refunded", "true")
        })),
    }]);
    let mut trace: Vec<TraceEvent> = Vec::new();
    let mut approvals: Vec<ApprovalRequest> = Vec::new();

    let report = run_agent(
        &mut k,
        &mut agent,
        &services(),
        &engine,
        None,
        None,
        "run_r3",
        "退款 u1",
        &mut trace,
        &mut approvals,
        &mut reject_all(),
    )
    .unwrap();

    assert_eq!(report.status, RunStatus::Failed);
    assert!(report.human_approval, "人工拒绝 → human_approval=true");
    assert!(
        !k.relations().any(|r| r.predicate == "refunded"),
        "世界无变化"
    );
    assert!(
        !trace.iter().any(|e| e.event_type == "MemoryUpdate"),
        "失败不沉淀记忆"
    );
}

// ── R4：未知能力决策 → Failed（无人工）────────────────────────────────
#[test]
fn r4_unknown_capability_fails() {
    let mut k = Kernel::new();
    let mut agent = spawn_agent(&mut k);
    let engine = FakeEngine::new(vec![PlanStep {
        capability: "mystery.cap".into(),
        args: json!({}),
        when: None,
    }]);
    let mut trace: Vec<TraceEvent> = Vec::new();
    let mut approvals: Vec<ApprovalRequest> = Vec::new();

    let report = run_agent(
        &mut k,
        &mut agent,
        &services(),
        &engine,
        None,
        None,
        "run_r4",
        "g",
        &mut trace,
        &mut approvals,
        &mut approve_all(),
    )
    .unwrap();

    assert_eq!(report.status, RunStatus::Failed);
    assert!(!report.human_approval);
    assert!(trace.iter().any(|e| e.event_type == "ActionFailed"));
}

// ── R5：EvolutionGate 放行 → 不经 resolver 直接完成 ──────────────────
#[test]
fn r5_evolution_gate_bypasses_resolver() {
    let mut k = Kernel::new();
    let mut agent = spawn_agent(&mut k);
    let engine = FakeEngine::new(vec![PlanStep {
        capability: "customer.refund".into(),
        args: json!({ "target": "u1", "amount": 100 }),
        when: Some(Box::new(|v: &AgentView| {
            !v.has_relation("refunded", "true")
        })),
    }]);
    // 种子：5 次成功 → Validated + auto_approve_enabled
    let mut ev = EvolutionEngine::new().with_gate_enabled(true);
    for _ in 0..5 {
        ev.record("RefundCustomer", OutcomeKind::Success);
    }
    ev.mine();
    ev.propose();
    ev.validate();
    assert!(ev.gate("RefundCustomer", Reversibility::Compensatable));

    let mut trace: Vec<TraceEvent> = Vec::new();
    let mut approvals: Vec<ApprovalRequest> = Vec::new();
    let mut resolver_calls = 0;

    let report = run_agent(
        &mut k,
        &mut agent,
        &services(),
        &engine,
        None,
        Some(&mut ev),
        "run_r5",
        "退款 u1",
        &mut trace,
        &mut approvals,
        &mut |_req| {
            resolver_calls += 1;
            ApprovalDecision::Approve { by: "x".into() }
        },
    )
    .unwrap();

    assert_eq!(report.outcome, LoopOutcome::Completed);
    assert_eq!(resolver_calls, 0, "Gate 放行不得惊动 resolver");
    assert!(k.relations().any(|r| r.predicate == "refunded"));
}

// ── R6：resolver Expire → Failed + human，审计 approval.expired ─────────
#[test]
fn r6_expire_decision_fails_with_expired_audit() {
    let mut k = Kernel::new();
    let mut agent = spawn_agent(&mut k);
    let engine = FakeEngine::new(vec![PlanStep {
        capability: "customer.refund".into(),
        args: json!({ "target": "u1", "amount": 100 }),
        when: Some(Box::new(|v: &AgentView| {
            !v.has_relation("refunded", "true")
        })),
    }]);
    let mut trace: Vec<TraceEvent> = Vec::new();
    let mut approvals: Vec<ApprovalRequest> = Vec::new();

    let report = run_agent(
        &mut k,
        &mut agent,
        &services(),
        &engine,
        None,
        None,
        "run_r6",
        "退款 u1",
        &mut trace,
        &mut approvals,
        &mut |_req| ApprovalDecision::Expire {
            by: "timeout".into(),
        },
    )
    .unwrap();

    assert_eq!(report.status, RunStatus::Failed);
    assert!(report.human_approval);
    assert!(
        k.events().iter().any(|e| e.ty == "approval.expired"),
        "Expire 裁决应落 approval.expired 审计（与 Rejected 区分）"
    );
    assert!(!k.relations().any(|r| r.predicate == "refunded"));
}
