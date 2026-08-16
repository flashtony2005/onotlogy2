//! M1b · G2 契约测试：审批适配器原语（v2_parity）。
//!
//! 契约：
//! - Waiting 时 AgentLoop 把类型化 ApprovalRequest 推入调用方 sink
//!   （run_id/capability/action/args/fiber 绑定，供 CLI/HTTP 适配器查询）。
//! - resolve(Approve) → Approved + 续跑完成（世界达成）；resolve(Reject) → Rejected +
//!   Cancelled（世界无变化）；已裁决请求拒绝二次裁决（AlreadyResolved）。
//! - expire → Expired（audit 事件 approval.expired，与 Rejected 区分）；二次裁决拒绝。
//! - DecisionCache：同 key（action@target）只询问一次，复用缓存决策。

use openaip_os::cognitive::approval::{
    ApprovalDecision, ApprovalRequest, ApprovalResolution, DecisionCache, expire, resolve,
};
use openaip_os::cognitive::engine::{AgentView, FakeEngine, PlanStep};
use openaip_os::cognitive::{Agent, AgentLoop, LoopOutcome};
use openaip_os::compose::policy::{Cmp, Policy, Precondition};
use openaip_os::compose::{Capability, Service, Task};
use openaip_os::kernel::effect::Reversibility;
use openaip_os::kernel::{Kernel, Scope};
use serde_json::json;

fn crm_service() -> Service {
    Service::new("crm-service").with_capability(
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

fn refund_engine() -> FakeEngine {
    FakeEngine::new(vec![PlanStep {
        capability: "customer.refund".into(),
        args: json!({ "target": "u1", "amount": 500 }),
        when: Some(Box::new(|v: &AgentView| {
            !v.has_relation("refunded", "true")
        })),
    }])
}

fn refund_task() -> Task {
    Task::new("refund", "退款 500").expecting(&[("refunded", "true")])
}

// ── G2a：Waiting 时注册类型化请求 + Approve 恢复完成 ──────────────────
#[test]
fn g2a_waiting_registers_request_then_approve_completes() {
    let mut k = Kernel::new();
    let mut agent = spawn_agent(&mut k);
    let services = [crm_service()];
    let engine = refund_engine();
    let task = refund_task();
    let ctx = agent.context;

    let mut requests: Vec<ApprovalRequest> = Vec::new();
    let out = {
        let mut lp = AgentLoop::new(&mut k, &mut agent, &services, &engine)
            .with_approvals("run_a1", &mut requests);
        lp.run_until_done(ctx, &task, json!({})).unwrap()
    };
    assert_eq!(out, LoopOutcome::Waiting);

    assert_eq!(requests.len(), 1, "挂起应注册 1 条审批请求");
    let req = &requests[0];
    assert_eq!(req.run_id, "run_a1");
    assert_eq!(req.capability, "customer.refund");
    assert_eq!(req.action, "RefundCustomer");
    assert_eq!(req.args, json!({ "target": "u1", "amount": 500 }));
    assert_eq!(req.context, ctx);

    // 审批通过 → 续跑完成（世界达成）
    let res = resolve(
        &mut k,
        req,
        &ApprovalDecision::Approve { by: "admin".into() },
    )
    .unwrap();
    assert_eq!(res, ApprovalResolution::Approved);
    let out2 = {
        let mut lp = AgentLoop::new(&mut k, &mut agent, &services, &engine);
        lp.resume_until_done(req.fiber, &task).unwrap()
    };
    assert_eq!(out2, LoopOutcome::Completed);
    assert!(k.relations().any(|r| r.predicate == "refunded"));
}

// ── G2b：Reject → Cancelled + 世界无变化 + 拒绝二次裁决 ───────────────
#[test]
fn g2b_reject_cancels_and_blocks_second_adjudication() {
    let mut k = Kernel::new();
    let mut agent = spawn_agent(&mut k);
    let services = [crm_service()];
    let engine = refund_engine();
    let task = refund_task();
    let ctx = agent.context;

    let mut requests: Vec<ApprovalRequest> = Vec::new();
    let out = {
        let mut lp = AgentLoop::new(&mut k, &mut agent, &services, &engine)
            .with_approvals("run_a2", &mut requests);
        lp.run_until_done(ctx, &task, json!({})).unwrap()
    };
    assert_eq!(out, LoopOutcome::Waiting);
    let req = requests[0].clone();

    let res = resolve(
        &mut k,
        &req,
        &ApprovalDecision::Reject {
            by: "compliance".into(),
        },
    )
    .unwrap();
    assert_eq!(res, ApprovalResolution::Rejected);
    assert_eq!(
        k.fiber(req.fiber).map(|f| f.state),
        Some(openaip_os::kernel::FiberState::Cancelled)
    );
    assert!(k.events().iter().any(|e| e.ty == "approval.rejected"));
    // 世界无变化（等待期间未执行）
    assert!(!k.relations().any(|r| r.predicate == "refunded"));
    // 已裁决 → 拒绝二次裁决；不可恢复
    let res2 = resolve(
        &mut k,
        &req,
        &ApprovalDecision::Approve { by: "admin".into() },
    )
    .unwrap();
    assert_eq!(res2, ApprovalResolution::AlreadyResolved);
    assert!(k.resume(req.fiber).is_err());
}

// ── G2c：Expire → Expired（与 Rejected 区分）+ 拒绝二次裁决 ───────────
#[test]
fn g2c_expire_is_distinct_from_reject() {
    let mut k = Kernel::new();
    let mut agent = spawn_agent(&mut k);
    let services = [crm_service()];
    let engine = refund_engine();
    let task = refund_task();
    let ctx = agent.context;

    let mut requests: Vec<ApprovalRequest> = Vec::new();
    let out = {
        let mut lp = AgentLoop::new(&mut k, &mut agent, &services, &engine)
            .with_approvals("run_a3", &mut requests);
        lp.run_until_done(ctx, &task, json!({})).unwrap()
    };
    assert_eq!(out, LoopOutcome::Waiting);
    let req = requests[0].clone();

    let res = expire(&mut k, &req).unwrap();
    assert_eq!(res, ApprovalResolution::Expired);
    assert_eq!(
        k.fiber(req.fiber).map(|f| f.state),
        Some(openaip_os::kernel::FiberState::Cancelled)
    );
    // 审计与 Rejected 区分（V1 tc023d 语义）
    assert!(k.events().iter().any(|e| e.ty == "approval.expired"));
    assert!(!k.events().iter().any(|e| e.ty == "approval.rejected"));
    // 已裁决 → 拒绝二次裁决
    let res2 = resolve(&mut k, &req, &ApprovalDecision::Reject { by: "x".into() }).unwrap();
    assert_eq!(res2, ApprovalResolution::AlreadyResolved);
}

// ── G2d：DecisionCache 同 key 只询问一次 ──────────────────────────────
#[test]
fn g2d_decision_cache_asks_once_per_key() {
    let mut asks = 0;
    let mut cache = DecisionCache::new();
    let args = json!({ "target": "u1", "amount": 500 });
    let key = DecisionCache::key("RefundCustomer", &args);
    assert_eq!(key, "RefundCustomer@u1");

    let d1 = cache.get_or_ask(&key, || {
        asks += 1;
        ApprovalDecision::Approve { by: "admin".into() }
    });
    let d2 = cache.get_or_ask(&key, || {
        asks += 1;
        ApprovalDecision::Reject { by: "ops".into() }
    });
    assert_eq!(d1, d2, "同 key 应复用首次决策");
    assert_eq!(asks, 1, "闭包只应询问一次");
}
