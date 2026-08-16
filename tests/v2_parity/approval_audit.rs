//! M1b · G4 契约测试：审批审计事实（人工审批路径补齐）。
//!
//! 契约（REQ-018 可审计承诺在 V2 的完整形态）：
//! - 显式 `k.approve` → `approval.granted`（by=审批人）——已存在（tc 覆盖）。
//! - 原始 `relate(Approval)` 恢复路径（外部事件统一入口）也必须落 `approval.granted`
//!   审计（by=关系 source），不留「审批发生了但事实层无记录」的洞。
//! - 全链审计种类：approval.requested / approval.granted / approval.rejected /
//!   approval.expired / evolution.gate_approved 各司其职，可回放。

use openaip_os::cognitive::engine::{AgentView, FakeEngine, PlanStep};
use openaip_os::cognitive::{Agent, AgentLoop, LoopOutcome};
use openaip_os::compose::policy::Policy;
use openaip_os::compose::{Capability, Service, Task};
use openaip_os::kernel::effect::Reversibility;
use openaip_os::kernel::relation::Relation;
use openaip_os::kernel::{Kernel, Scope};
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

// ── G4a：relate(Approval) 恢复路径落 approval.granted 审计 ─────────────
#[test]
fn g4a_raw_relate_approval_audits_grant() {
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
    let out = {
        let mut lp = AgentLoop::new(&mut k, &mut agent, &services, &engine);
        lp.run_until_done(ctx, &task, json!({})).unwrap()
    };
    assert_eq!(out, LoopOutcome::Waiting);

    // 审批前：requested 已审计
    assert!(k.events().iter().any(|e| e.ty == "approval.requested"));

    // 原始 relate（A2 风格，不经过 k.approve）→ 必须落 granted 审计
    k.relate(Relation::new(ctx, "alice", "Approval", "admin", k.now()))
        .unwrap();
    assert!(
        k.events().iter().any(|e| e.ty == "approval.granted"
            && e.payload.get("by").and_then(|v| v.as_str()) == Some("alice")),
        "relate 恢复必须落 approval.granted（by=关系 source）: {:?}",
        k.events()
    );
}

// ── G4b：显式 k.approve 路径 granted 审计（by=审批人）─────────────────
#[test]
fn g4b_explicit_approve_audits_grant_with_approver() {
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
    let mut lp = AgentLoop::new(&mut k, &mut agent, &services, &engine);
    let out = lp.run_until_done(ctx, &task, json!({})).unwrap();
    assert_eq!(out, LoopOutcome::Waiting);
    drop(lp);
    let fiber = k
        .all_fibers()
        .find(|f| f.context == ctx && f.task == task.id)
        .unwrap()
        .id;

    k.approve(fiber, "admin").unwrap();
    assert!(
        k.events().iter().any(|e| e.ty == "approval.granted"
            && e.payload.get("by").and_then(|v| v.as_str()) == Some("admin")),
        "显式 approve 应落 granted（by=admin）"
    );
    // 续跑完成
    let out2 = {
        let mut lp = AgentLoop::new(&mut k, &mut agent, &services, &engine);
        lp.resume_until_done(fiber, &task).unwrap()
    };
    assert_eq!(out2, LoopOutcome::Completed);
}

// ── G4c：拒绝/超时审计种类齐全（与 granted 区分，可回放）─────────────
#[test]
fn g4c_reject_and_expire_audit_kinds_distinct() {
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
    let mut lp = AgentLoop::new(&mut k, &mut agent, &services, &engine);
    let out = lp.run_until_done(ctx, &task, json!({})).unwrap();
    assert_eq!(out, LoopOutcome::Waiting);
    drop(lp);
    let fiber = k
        .all_fibers()
        .find(|f| f.context == ctx && f.task == task.id)
        .unwrap()
        .id;

    k.reject_approval(fiber, "compliance").unwrap();
    assert!(k.events().iter().any(|e| e.ty == "approval.rejected"));

    // 第二个 fiber 走超时
    let mut agent2 = spawn_agent(&mut k);
    let ctx2 = agent2.context;
    let mut lp2 = AgentLoop::new(&mut k, &mut agent2, &services, &engine);
    let out2 = lp2.run_until_done(ctx2, &task, json!({})).unwrap();
    assert_eq!(out2, LoopOutcome::Waiting);
    drop(lp2);
    let fiber2 = k
        .all_fibers()
        .find(|f| f.context == ctx2 && f.task == task.id)
        .unwrap()
        .id;
    k.expire_approval(fiber2, "timeout").unwrap();
    assert!(k.events().iter().any(|e| e.ty == "approval.expired"));
}
