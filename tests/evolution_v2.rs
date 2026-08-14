//! V2.0 Evolution 自进化（Phase 6）集成契约。
//!
//! 核心不变量：**Evolution learns from execution, but learning never becomes authority.**
//! `Pattern ≠ Permission / Policy / Capability`。
//!
//! E1 Fiber outcome 正确采集（四类分账）
//! E2 Success / Failure Pattern 提炼（双通道）
//! E3 SuccessPattern → Proposal（仅此通道可产生）
//! E4 FailurePattern → AgentView.evolution（RiskSignal 注入）
//! E5 EvolutionGate 七条件 AND（任一缺失 → WAIT 非 DENY）
//! E6 Gate 通过 → 审计事实（evolution.gate_approved）
//! E7 Revoked 后永久不能自动 Gate（红线验证）
//! E8 End-to-end：经验 → Pattern → Cognitive/Permission → Agent 闭环
//! 反向：I1 Pattern 本身不能直接产生 Permission；I2 FailurePattern 不修改 Policy

use openaip_os::cognitive::engine::{AgentView, FakeEngine, PlanStep};
use openaip_os::cognitive::observe::observe;
use openaip_os::cognitive::{Agent, AgentLoop, LoopOutcome, Memory};
use openaip_os::compose::policy::{Cmp, Policy, PolicyVerdict, Precondition};
use openaip_os::compose::{Capability, Service, Task, TaskExecutor};
use openaip_os::evolution2::gate::GateVerdict;
use openaip_os::evolution2::{EvolutionEngine, EvolutionPermissionState, OutcomeKind, Pattern};
use openaip_os::kernel::effect::Reversibility;
use openaip_os::kernel::relation::Relation;
use openaip_os::kernel::{Kernel, Scope};
use serde_json::json;

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

/// 造一份 refund 经验（成功 n 次 → 提炼 → 提案 → 验证 → 可放行）。
fn seed_refund_success(n: u64) -> EvolutionEngine {
    let mut ev = EvolutionEngine::new().with_gate_enabled(true);
    for _ in 0..n {
        ev.record("RefundCustomer", OutcomeKind::Success);
    }
    ev.mine();
    ev.propose();
    ev.validate();
    ev
}

// ── E1：Fiber outcome 四类分账 ─────────────────────────────────────────
#[tokio::test]
async fn tc_e1_outcome_ledger() {
    let mut ev = EvolutionEngine::new();
    ev.record("RefundCustomer", OutcomeKind::Success);
    ev.record("RefundCustomer", OutcomeKind::Success);
    ev.record("RefundCustomer", OutcomeKind::Failed);
    ev.record("RefundCustomer", OutcomeKind::Compensated);
    ev.record("RefundCustomer", OutcomeKind::Denied);

    let exp = ev.experience("RefundCustomer").expect("经验");
    assert_eq!(exp.success, 2);
    assert_eq!(exp.failed, 1);
    assert_eq!(exp.compensated, 1);
    assert_eq!(exp.denied, 1);
    assert_eq!(exp.total(), 5);
    // 不同 action 隔离
    ev.record("RetainCustomer", OutcomeKind::Success);
    assert_eq!(ev.experience("RetainCustomer").unwrap().success, 1);
}

// ── E2：双通道 Pattern 提炼（幂等）────────────────────────────────────
#[tokio::test]
async fn tc_e2_pattern_mining() {
    let mut ev = EvolutionEngine::new();
    for _ in 0..5 {
        ev.record("RetainCustomer", OutcomeKind::Success);
    }
    ev.record("RefundCustomer", OutcomeKind::Failed);
    ev.record("RefundCustomer", OutcomeKind::Compensated);
    ev.mine();

    let success = ev
        .patterns()
        .iter()
        .find(|p| matches!(p, Pattern::Success(s) if s.action == "RetainCustomer"));
    assert!(
        matches!(success, Some(Pattern::Success(s)) if s.success == 5 && s.confidence == 1.0),
        "应有 SuccessPattern（confidence=1.0）"
    );
    let failure = ev
        .patterns()
        .iter()
        .find(|p| matches!(p, Pattern::Failure(f) if f.action == "RefundCustomer"));
    assert!(
        matches!(failure, Some(Pattern::Failure(f)) if f.failed == 1 && f.compensated == 1),
        "应有 FailurePattern（失败+补偿分账）"
    );
    // 幂等：重提炼结果一致
    let before: Vec<Pattern> = ev.patterns().to_vec();
    ev.mine();
    assert_eq!(ev.patterns(), &before[..]);
}

// ── E3：SuccessPattern → Proposal（幂等；Failure 通道不参与）───────────
#[tokio::test]
async fn tc_e3_success_pattern_proposes() {
    let mut ev = EvolutionEngine::new();
    for _ in 0..5 {
        ev.record("RetainCustomer", OutcomeKind::Success);
    }
    ev.record("RefundCustomer", OutcomeKind::Failed);
    ev.record("RefundCustomer", OutcomeKind::Failed);
    ev.mine();
    ev.propose();

    let prop = ev.proposal("RetainCustomer").expect("Proposal");
    assert_eq!(prop.id, "proposal.RetainCustomer", "幂等稳定 id");
    assert_eq!(prop.target, "RetainCustomer");
    assert_eq!(prop.confidence, 1.0);
    assert_eq!(
        prop.status,
        openaip_os::evolution2::ProposalStatus::Proposed
    );
    // FailurePattern 不产生 Proposal（I1）
    assert!(ev.proposal("RefundCustomer").is_none());
    // 幂等 upsert
    ev.propose();
    assert!(ev.proposals().len() <= 1, "不重复提案");
}

// ── E4：FailurePattern → AgentView.evolution（认知层注入）──────────────
#[tokio::test]
async fn tc_e4_failure_pattern_reaches_agent_view() {
    let mut k = Kernel::new();
    let ctx = k.create_context(None, Scope::World, "world").unwrap();
    let mut ev = EvolutionEngine::new();
    ev.record("RefundCustomer", OutcomeKind::Failed);
    ev.record("RefundCustomer", OutcomeKind::Compensated);
    ev.record("RefundCustomer", OutcomeKind::Compensated);
    ev.mine();

    let memory = Memory::new();
    let view = observe(
        &k,
        ctx,
        "reduce churn",
        &memory,
        &["customer.refund".to_string()],
        Some(&ev),
    );
    assert!(
        view.evolution.iter().any(|r| r.action == "RefundCustomer"),
        "AgentView.evolution 应含 RefundCustomer 风险信号"
    );
    let sig = view
        .evolution
        .iter()
        .find(|r| r.action == "RefundCustomer")
        .unwrap();
    assert_eq!(sig.failed, 1);
    assert_eq!(sig.compensated, 2);

    // 无演化 → 空注入
    let view0 = observe(&k, ctx, "goal", &memory, &[], None);
    assert!(view0.evolution.is_empty());
}

// ── E5：Gate 七条件 AND（engine.gate 集成层）───────────────────────────
#[tokio::test]
async fn tc_e5_gate_and_conditions() {
    // 全满足 → 放行
    let ev = seed_refund_success(5);
    assert!(
        ev.gate("RefundCustomer", Reversibility::Compensatable),
        "全部条件满足应放行"
    );

    // 逐个缺失（用 check 纯函数覆盖，与 gate.rs 单测互补）
    let ok = (
        true,
        EvolutionPermissionState::Validated,
        true,
        1.0,
        5u64,
        0u64,
        Reversibility::Compensatable,
    );
    let cases: Vec<(
        bool,
        EvolutionPermissionState,
        bool,
        f64,
        u64,
        u64,
        Reversibility,
    )> = vec![
        (false, ok.1, ok.2, ok.3, ok.4, ok.5, ok.6), // 全局开关关
        (
            true,
            EvolutionPermissionState::Revoked,
            false,
            ok.3,
            ok.4,
            ok.5,
            ok.6,
        ), // Revoked
        (
            true,
            EvolutionPermissionState::Candidate,
            true,
            ok.3,
            ok.4,
            ok.5,
            ok.6,
        ), // 未 Validated
        (true, ok.1, false, ok.3, ok.4, ok.5, ok.6), // auto_approve 关
        (true, ok.1, true, 0.7, ok.4, ok.5, ok.6),   // 置信 < 0.8
        (true, ok.1, true, ok.3, 2, ok.5, ok.6),     // 成功 < 3
        (true, ok.1, true, ok.3, ok.4, 1, ok.6),     // 有失败
        (true, ok.1, true, ok.3, ok.4, 0, Reversibility::Idempotent), // 不可补偿
        (true, ok.1, true, ok.3, ok.4, 0, Reversibility::Irreversible), // 不可逆
    ];
    for (i, c) in cases.iter().enumerate() {
        assert_eq!(
            openaip_os::evolution2::gate::check(c.0, c.1, c.2, c.3, c.4, c.5, c.6),
            GateVerdict::Wait,
            "case {i} 应 Wait（不能替代审批 ≠ 权限拒绝）"
        );
    }
}

// ── E6：Gate 通过 → 免人工审批 + 审计事实 ──────────────────────────────
#[tokio::test]
async fn tc_e6_gate_approves_and_audits() {
    let services = [crm_service()];
    let mut k = Kernel::new();
    let ctx = k.create_context(None, Scope::World, "world").unwrap();
    let ev = seed_refund_success(5);

    let cap = services[0]
        .capability("customer.refund")
        .expect("refund 能力")
        .clone();
    let task = Task::new("refund", "退款").expecting(&[("refunded", "true")]);
    let fiber = {
        let mut tx = TaskExecutor::new(&mut k);
        tx.spawn(ctx, &task, json!({})).unwrap()
    };
    let out = {
        let mut tx = TaskExecutor::new(&mut k);
        let gate_pass = ev.gate("RefundCustomer", cap.reversibility);
        tx.step_with_gate(
            fiber,
            &cap,
            json!({ "target": "u1", "amount": 100 }),
            gate_pass,
        )
        .unwrap()
    };
    assert_eq!(
        out,
        openaip_os::compose::StepOutcome::Committed,
        "Gate 放行 → 直接提交"
    );
    // 审计事实（可观察）
    assert!(
        k.events().iter().any(|e| e.ty == "evolution.gate_approved"),
        "应有 evolution.gate_approved 审计事件"
    );
    // 世界达成（投影照常）
    assert!(
        k.relations()
            .any(|r| r.predicate == "refunded" && r.target == "true")
    );
}

// ── E7：Revoked 后永久不能自动 Gate（红线验证）────────────────────────
#[tokio::test]
async fn tc_e7_revoked_is_permanent() {
    let services = [crm_service()];
    let mut k = Kernel::new();
    let ctx = k.create_context(None, Scope::World, "world").unwrap();
    let mut ev = seed_refund_success(5);
    assert!(ev.gate("RefundCustomer", Reversibility::Compensatable));

    // 撤销
    ev.revoke("RefundCustomer");
    assert_eq!(
        ev.permission("RefundCustomer").map(|p| p.state),
        Some(EvolutionPermissionState::Revoked)
    );
    assert!(
        !ev.gate("RefundCustomer", Reversibility::Compensatable),
        "撤销后不放行"
    );

    // 吸收态：再次验证也不复活（杜绝 enabled=false → 重新满足 → 自动复活）
    ev.validate();
    assert_eq!(
        ev.permission("RefundCustomer").map(|p| p.state),
        Some(EvolutionPermissionState::Revoked)
    );
    assert!(
        !ev.gate("RefundCustomer", Reversibility::Compensatable),
        "Revoked 永久禁止"
    );

    // 走真实 step → 回退人工（Waiting，不是 Deny）
    let cap = services[0].capability("customer.refund").unwrap().clone();
    let task = Task::new("refund", "退款").expecting(&[("refunded", "true")]);
    let fiber = {
        let mut tx = TaskExecutor::new(&mut k);
        tx.spawn(ctx, &task, json!({})).unwrap()
    };
    let out = {
        let mut tx = TaskExecutor::new(&mut k);
        tx.step_with_gate(fiber, &cap, json!({ "target": "u1", "amount": 100 }), false)
            .unwrap()
    };
    assert_eq!(
        out,
        openaip_os::compose::StepOutcome::Waiting,
        "Gate 失败 → 人工审批（WAIT 非 DENY）"
    );
}

// ── E8：端到端闭环 — 3 次人工审批成功 → 学习 → 第 4 次自动放行 ────────
#[tokio::test]
async fn tc_e8_end_to_end_learning_then_gate() {
    let services = [crm_service()];
    let mut k = Kernel::new();
    let mut agent = spawn_agent(&mut k);
    let ctx = agent.context;
    let mut ev = EvolutionEngine::new().with_gate_enabled(true);

    // ① 5 轮 refund，每轮人工审批（模拟真实审批流），经验累计 Success×5
    //    注：Gate 要求 Success≥3 **且** Confidence≥0.8 同时成立（min(1, success/5)），
    //    3 次仅 0.6；5 次 = 1.0 恰好两个条件都过。
    for i in 1..=5 {
        let target = format!("cust-0{i}");
        let engine = FakeEngine::new(vec![PlanStep {
            capability: "customer.refund".into(),
            args: json!({ "target": target.clone(), "amount": 100 }),
            when: Some(Box::new(move |v: &AgentView| {
                !v.related
                    .iter()
                    .any(|(s, p, _)| s == &target && p == "refunded")
            })),
        }]);
        let task = Task::new(format!("refund-{i}"), "退款").expecting(&[("refunded", "true")]);
        let out = {
            let mut loop_ =
                AgentLoop::new(&mut k, &mut agent, &services, &engine).with_evolution(&mut ev);
            loop_.run_until_done(ctx, &task, json!({})).unwrap()
        };
        assert_eq!(out, LoopOutcome::Waiting, "学习前应等待人工审批");
        // 人工审批 → 恢复 → 完成
        let fiber = k
            .all_fibers()
            .find(|f| f.context == ctx && f.task == format!("refund-{i}"))
            .expect("task fiber")
            .id;
        k.relate(Relation::new(ctx, "approval", "Approval", "admin", k.now()))
            .unwrap();
        let out2 = {
            let mut loop_ =
                AgentLoop::new(&mut k, &mut agent, &services, &engine).with_evolution(&mut ev);
            loop_.resume_until_done(fiber, &task).unwrap()
        };
        assert_eq!(out2, LoopOutcome::Completed);
    }
    // 经验分账正确
    assert_eq!(ev.experience("RefundCustomer").unwrap().success, 5);
    assert_eq!(ev.experience("RefundCustomer").unwrap().denied, 0);
    // 置信度门槛：成功≥3 但置信<0.8 时不得放行（两条件缺一不可）
    let mut ev60 = EvolutionEngine::new().with_gate_enabled(true);
    for _ in 0..3 {
        ev60.record("RetainCustomer", OutcomeKind::Success);
    }
    ev60.mine();
    ev60.propose();
    ev60.validate();
    assert!(
        !ev60.gate("RetainCustomer", Reversibility::Compensatable),
        "Success=3 但 Confidence=0.6<0.8 → 不放行"
    );

    // ② 提炼 → 提案 → 验证 → 获得放行资格
    ev.mine();
    ev.propose();
    ev.validate();
    assert_eq!(
        ev.permission("RefundCustomer").map(|p| p.state),
        Some(EvolutionPermissionState::Validated)
    );
    assert!(ev.gate("RefundCustomer", Reversibility::Compensatable));

    // ③ 第 6 轮：无需人工审批，直接完成（EvolutionGate 替代人工）
    let target6 = "cust-06".to_string();
    let engine6 = FakeEngine::new(vec![PlanStep {
        capability: "customer.refund".into(),
        args: json!({ "target": target6.clone(), "amount": 100 }),
        when: Some(Box::new(move |v: &AgentView| {
            !v.related
                .iter()
                .any(|(s, p, _)| s == &target6 && p == "refunded")
        })),
    }]);
    let task6 = Task::new("refund-6", "退款").expecting(&[("refunded", "true")]);
    let out4 = {
        let mut loop_ =
            AgentLoop::new(&mut k, &mut agent, &services, &engine6).with_evolution(&mut ev);
        loop_.run_until_done(ctx, &task6, json!({})).unwrap()
    };
    assert_eq!(out4, LoopOutcome::Completed, "学习后应自动放行");
    // 审计事实（Learning ≠ Authority 的可审计证据）
    assert!(
        k.events().iter().any(|e| e.ty == "evolution.gate_approved"),
        "端到端应有 gate_approved 审计"
    );
    // 世界真实达成
    assert!(
        k.relations()
            .any(|r| r.source == "cust-06" && r.predicate == "refunded")
    );
}

// ── 反向 I1：Pattern 本身不能直接产生 Permission ──────────────────────
#[tokio::test]
async fn tc_inv1_pattern_is_not_permission() {
    let mut ev = EvolutionEngine::new().with_gate_enabled(true);
    for _ in 0..5 {
        ev.record("RetainCustomer", OutcomeKind::Success);
    }
    // 只有 Pattern：无 Proposal → 不放行
    ev.mine();
    assert!(
        !ev.gate("RetainCustomer", Reversibility::Compensatable),
        "Pattern ≠ Permission"
    );
    // Proposal 但未 Validation → 不放行
    ev.propose();
    assert!(
        !ev.gate("RetainCustomer", Reversibility::Compensatable),
        "未验证不放行"
    );
    // 必经 Proposal → Validation → Gate 全链路
    ev.validate();
    assert!(ev.gate("RetainCustomer", Reversibility::Compensatable));
}

// ── 反向 I2：FailurePattern 不修改 Policy、不产生放行 ─────────────────
#[tokio::test]
async fn tc_inv2_failure_pattern_never_policy() {
    let services = [crm_service()];
    let cap = services[0].capability("customer.refund").unwrap();
    let args = json!({ "amount": 2000 });

    // Policy 检查基准（amount 2000 > 1000 → 前置条件拒绝）
    let before = cap.policy.check(&args);

    // 构造失败模式（认知信号）
    let mut ev = EvolutionEngine::new().with_gate_enabled(true);
    ev.record("RefundCustomer", OutcomeKind::Failed);
    ev.record("RefundCustomer", OutcomeKind::Compensated);
    ev.record("RefundCustomer", OutcomeKind::Compensated);
    ev.mine();
    assert!(
        ev.risk_signals()
            .iter()
            .any(|r| r.action == "RefundCustomer"),
        "失败模式进认知层"
    );

    // Policy 未被修改（同一判定）
    let after = cap.policy.check(&args);
    assert_eq!(
        format!("{before:?}"),
        format!("{after:?}"),
        "FailurePattern 不得修改 Policy（I2）"
    );
    assert!(matches!(after, PolicyVerdict::Deny(_)));

    // FailurePattern 不产生任何放行资格
    assert!(
        !ev.gate("RefundCustomer", Reversibility::Compensatable),
        "失败经验绝不能 Gate 放行"
    );
}
