//! V2.0 认知层（Phase 3）集成契约：Agent Loop 闭环。
//!
//! A1 闭环：Observe→Decide→Act→Verify（expected_relations 目标函数）
//! A2 审批暂停恢复：Waiting → explicit approve → resume_until_done
//! A3 Memory 沉淀/召回（D3）
//! A4 前置条件 Deny（业务拒绝）
//! A5 无决策且目标未达成 → Failed（不误判完成）
//! A6 Service 绑定 Context（③）
//! A7 required_params 缺参校验（②）

use openaip_os::cognitive::engine::{AgentView, FakeEngine, NoOpEngine, PlanStep};
use openaip_os::cognitive::{Agent, AgentLoop, LoopOutcome};
use openaip_os::compose::policy::{Cmp, Policy, Precondition};
use openaip_os::compose::{Capability, Service, Task};
use openaip_os::kernel::effect::Reversibility;
use openaip_os::kernel::{Kernel, Scope};
use serde_json::{Value, json};

/// CRM 服务：activate（免审批 + 投影 status→active）、refund（审批 + 前置条件 + 补偿）、retain。
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
        .with_capability(
            Capability::new("customer.retain", "RetainCustomer")
                .with_projection("retained", "true")
                .requiring(&["target"]),
        )
}

fn spawn_agent(k: &mut Kernel) -> Agent {
    let world = k.create_context(None, Scope::World, "world").unwrap();
    Agent::new("retention-agent", world)
}

/// 完整任务循环（helper：函数内短暂借用，返回后 k/agent 可自由使用）。
fn run_task(
    k: &mut Kernel,
    agent: &mut Agent,
    services: &[Service],
    engine: &dyn openaip_os::cognitive::CognitiveEngine,
    task: &Task,
    input: Value,
) -> LoopOutcome {
    let ctx = agent.context;
    let mut loop_ = AgentLoop::new(k, agent, services, engine);
    loop_.run_until_done(ctx, task, input).unwrap()
}

// ── A1：闭环 — Observe→Decide→Act→Verify（D5 目标函数） ──────────────────
#[tokio::test]
async fn tc_a1_agent_closed_loop_verify() {
    let services = [crm_service()];
    let mut k = Kernel::new();
    let mut agent = spawn_agent(&mut k);
    let engine = FakeEngine::new(vec![PlanStep {
        capability: "customer.activate".into(),
        args: json!({ "target": "user-001" }),
        when: Some(Box::new(|v: &AgentView| {
            !v.has_relation("status", "active")
        })),
    }]);

    // 任务：激活用户；完成判定 = 世界出现 User --status--> active
    let task = Task::new("activate-user", "激活用户 Mason").expecting(&[("status", "active")]);
    let out = run_task(&mut k, &mut agent, &services, &engine, &task, json!({}));
    assert_eq!(out, LoopOutcome::Completed);

    // Verify：世界确实达成目标（不是 Agent 自称）
    assert!(
        k.relations()
            .any(|r| r.predicate == "status" && r.target == "active")
    );
    // Memory 沉淀（D3）
    assert_eq!(agent.memory.hits_of("ActivateCustomer"), 1);
    // 事实层可审计
    assert!(k.events().iter().any(|e| e.ty == "effect.committed"));
}

// ── A2：审批暂停恢复 — Waiting → explicit approve → 续跑完成 ────────────
#[tokio::test]
async fn tc_a2_approval_pause_and_resume() {
    let services = [crm_service()];
    let mut k = Kernel::new();
    let mut agent = spawn_agent(&mut k);
    let engine = FakeEngine::new(vec![PlanStep {
        capability: "customer.refund".into(),
        args: json!({ "target": "user-001", "amount": 500 }),
        when: Some(Box::new(|v: &AgentView| {
            !v.has_relation("refunded", "true")
        })),
    }]);

    let task = Task::new("refund", "退款 500").expecting(&[("refunded", "true")]);
    let out = run_task(&mut k, &mut agent, &services, &engine, &task, json!({}));
    assert_eq!(out, LoopOutcome::Waiting, "require_approval 应挂起");

    // 人工审批 → 显式恢复 → 续跑完成（重建 loop 借用）
    let fiber = task_fiber_of(&k, &agent, &task);
    k.approve(fiber, "admin").unwrap();
    let out2 = {
        let mut loop_ = AgentLoop::new(&mut k, &mut agent, &services, &engine);
        loop_.resume_until_done(fiber, &task).unwrap()
    };
    assert_eq!(out2, LoopOutcome::Completed);
    assert!(k.relations().any(|r| r.predicate == "refunded"));
}

/// 找 agent context 下该 task 的 fiber（审批恢复后继续同一实例）。
fn task_fiber_of(k: &Kernel, agent: &Agent, task: &Task) -> openaip_os::kernel::FiberId {
    k.all_fibers()
        .find(|f| f.context == agent.context && f.task == task.id)
        .expect("task fiber 应存在")
        .id
}

// ── A3：Memory 沉淀/召回 — 第二步决策能看到第一步的记忆（D3） ────────────
#[tokio::test]
async fn tc_a3_memory_recall_influences_decision() {
    let services = [crm_service()];
    let mut k = Kernel::new();
    let mut agent = spawn_agent(&mut k);

    // 第二步只有当 view.memory 含 RetainCustomer 记忆才触发（验证召回注入）
    let engine = FakeEngine::new(vec![
        PlanStep {
            capability: "customer.retain".into(),
            args: json!({ "target": "user-001" }),
            // 目标达成前持续行动（D5）：retained 关系出现后不再触发
            when: Some(Box::new(|v: &AgentView| {
                !v.has_relation("retained", "true")
            })),
        },
        PlanStep {
            capability: "customer.activate".into(),
            args: json!({ "target": "user-001" }),
            when: Some(Box::new(|v: &AgentView| {
                v.memory.iter().any(|m| m.label == "RetainCustomer")
                    && !v.has_relation("status", "active")
            })),
        },
    ]);
    let task = Task::new("retain-then-activate", "挽留并激活").expecting(&[("status", "active")]);
    let out = run_task(&mut k, &mut agent, &services, &engine, &task, json!({}));
    assert_eq!(out, LoopOutcome::Completed);
    // 两步都执行（第二步依赖第一步沉淀的记忆）
    assert!(k.relations().any(|r| r.predicate == "retained"));
    assert!(
        k.relations()
            .any(|r| r.predicate == "status" && r.target == "active")
    );
    assert_eq!(agent.memory.hits_of("RetainCustomer"), 1);
    assert_eq!(agent.memory.hits_of("ActivateCustomer"), 1);
}

// ── A4：前置条件 Deny — 业务拒绝（无 Effect，空补偿收尾） ─────────────────
#[tokio::test]
async fn tc_a4_precondition_deny() {
    let services = [crm_service()];
    let mut k = Kernel::new();
    let mut agent = spawn_agent(&mut k);
    let engine = FakeEngine::new(vec![PlanStep {
        capability: "customer.refund".into(),
        args: json!({ "target": "user-001", "amount": 5000 }),
        when: None,
    }]);
    let task = Task::new("refund-big", "大额退款").expecting(&[("refunded", "true")]);
    let out = run_task(&mut k, &mut agent, &services, &engine, &task, json!({}));
    assert!(matches!(out, LoopOutcome::Denied(_)));
    // 无 Effect 提交；policy.denied 事件留痕
    assert!(k.events().iter().any(|e| e.ty == "policy.denied"));
}

// ── A5：无决策且目标未达成 → Failed（不误判完成） ─────────────────────────
#[tokio::test]
async fn tc_a5_no_decision_no_complete() {
    let services = [crm_service()];
    let mut k = Kernel::new();
    let mut agent = spawn_agent(&mut k);
    let engine = NoOpEngine;
    let task = Task::new("never", "无法完成的任务").expecting(&[("status", "active")]);
    let out = run_task(&mut k, &mut agent, &services, &engine, &task, json!({}));
    assert!(matches!(out, LoopOutcome::Failed(_)));
    // 空期望关系 → 直接完成（边界）
    let task2 = Task::new("empty", "无目标任务");
    let f = run_task(&mut k, &mut agent, &services, &engine, &task2, json!({}));
    assert_eq!(f, LoopOutcome::Completed, "无期望关系且无决策 = 完成");
}

// ── A6：Service 绑定 Context（③） ─────────────────────────────────────────
#[tokio::test]
async fn tc_a6_service_bind_context() {
    let svc = crm_service();
    let mut k = Kernel::new();
    let world = k.create_context(None, Scope::World, "world").unwrap();
    svc.bind(&mut k, world).unwrap();
    assert!(
        k.relations()
            .any(|r| r.predicate == "serves" && r.source == "service:crm-service"),
        "应记录 Service --serves--> Context"
    );
}

// ── A7：required_params 缺参 → Denied（② schema 校验） ───────────────────
#[tokio::test]
async fn tc_a7_required_params_validation() {
    let services = [crm_service()];
    let mut k = Kernel::new();
    let mut agent = spawn_agent(&mut k);
    let engine = FakeEngine::new(vec![PlanStep {
        capability: "customer.activate".into(),
        args: json!({}),
        when: None,
    }]); // 缺 target
    let task = Task::new("activate", "激活").expecting(&[("status", "active")]);
    let out = run_task(&mut k, &mut agent, &services, &engine, &task, json!({}));
    assert!(matches!(out, LoopOutcome::Denied(reason) if reason.contains("target")));
    assert!(k.events().iter().any(|e| e.ty == "policy.denied"));
    // 无 Effect（参数校验失败不产生副作用）
    assert!(k.all_effects().next().is_none());
}
