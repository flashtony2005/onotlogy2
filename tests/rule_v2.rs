//! V2.0 Self-Evolving Agent（Phase 7）集成契约。
//!
//! 第一原则：**Self-Evolution must follow the same execution path as
//! ordinary world evolution.** —— 行为演化无后门路径。
//!
//! F1 行为即数据：RuleStore / ConditionExpr 序列化 roundtrip
//! F2 RuleEngine 决策：结构化条件（HasRelation/HasRisk/Not）+ priority + enabled
//! F3 同一路径：RuleEngine 进 AgentLoop 全流程（审批/Verify 零 bypass）
//! F4 规则级经验分账（rule_stats 与 action 级并存——双尺度）
//! F5 BehaviorProposal 提炼：高频失败 → Disable；高频成功 → Promote
//! F6 Validation 门槛：样本/置信不足 → 不 adopt
//! F7 Adoption 后行为改变：停用规则不再被选 / 提升规则优先被选
//! F8 端到端：stats → 提案 → 治理验证 → adopt → 行为改变 → 审计 → 撤销恢复
//! F-Rev1 无后门：未 Validated 无法 adopt；行为变化必有审计
//! F-Rev2 不侵 World：adopt 前后 kernel 事件/关系零异常
//! F-Rev3 no-dead-agent：Disable 最后一个可行规则 → reject
//! F-Rev4 stale：v3 提案 → 规则变 v4 → adopt reject

use openaip_os::cognitive::observe::observe;
use openaip_os::cognitive::{Agent, AgentLoop, CognitiveEngine, LoopOutcome, Memory};
use openaip_os::compose::policy::{Cmp, Policy, Precondition};
use openaip_os::compose::{Capability, Service, Task};
use openaip_os::evolution2::{EvolutionEngine, OutcomeKind};
use openaip_os::kernel::effect::Reversibility;
use openaip_os::kernel::relation::Relation;
use openaip_os::kernel::{Kernel, Scope};
use openaip_os::rule::{
    BehaviorChange, BehaviorProposal, BehaviorRule, ConditionExpr, RuleEngine, RuleError,
    RuleEvolver, RuleStatsStore, RuleStore, ValidationContext, ValidationStatus, propose_changes,
    validate,
};
use serde_json::json;

/// CRM 服务（同 evolution_v2：activate 免审批 / refund 审批+前置条件 / retain 投影）。
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

fn caps_of() -> Vec<String> {
    vec![
        "customer.activate".into(),
        "customer.refund".into(),
        "customer.retain".into(),
    ]
}

fn validate_ctx(rules: Vec<BehaviorRule>) -> ValidationContext {
    ValidationContext {
        rules,
        capabilities: caps_of(),
        policy_fingerprint: "policy-v1".into(),
        ontology_fingerprint: "ontology-v1".into(),
    }
}

/// 条件：世界尚无 status→active。
fn not_active() -> ConditionExpr {
    ConditionExpr::Not(Box::new(ConditionExpr::HasRelation {
        predicate: "status".into(),
        target: "active".into(),
    }))
}

// ── F1：行为即数据（序列化 ABI）───────────────────────────────────────
#[tokio::test]
async fn tc_f1_behavior_is_data() {
    let store = RuleStore::new()
        .with_rule(
            BehaviorRule::new(
                "r1",
                "customer.activate",
                json!({ "target": "u1" }),
                ConditionExpr::All(vec![
                    not_active(),
                    ConditionExpr::Not(Box::new(ConditionExpr::HasRisk {
                        action: "RefundCustomer".into(),
                    })),
                ]),
            )
            .with_priority(10),
        )
        .with_rule(BehaviorRule::new(
            "r2",
            "customer.retain",
            json!({ "target": "u2" }),
            ConditionExpr::Any(vec![
                ConditionExpr::HasRelation {
                    predicate: "status".into(),
                    target: "active".into(),
                },
                ConditionExpr::HasRelation {
                    predicate: "retained".into(),
                    target: "true".into(),
                },
            ]),
        ));
    let s = serde_json::to_string(&store).unwrap();
    let back: RuleStore = serde_json::from_str(&s).unwrap();
    assert_eq!(store, back, "行为 ABI roundtrip");
    assert!(back.rule("r1").is_some());
}

// ── F2：RuleEngine 结构化决策（含 HasRisk 条件）───────────────────────
#[tokio::test]
async fn tc_f2_rule_engine_structured_decisions() {
    let mut k = Kernel::new();
    let ctx = k.create_context(None, Scope::World, "w").unwrap();
    let t0 = k.now();
    k.relate(Relation::new(ctx, "cust-01", "status", "active", t0))
        .unwrap();

    // 风险信号：RefundCustomer 高频失败 → AgentView.evolution 注入
    let mut ev = EvolutionEngine::new();
    ev.record("RefundCustomer", OutcomeKind::Failed);
    ev.record("RefundCustomer", OutcomeKind::Compensated);
    ev.record("RefundCustomer", OutcomeKind::Compensated);
    ev.mine();

    // 规则集：无风险 → retain；有风险 → refund（HasRisk 条件驱动决策）
    let store = RuleStore::new()
        .with_rule(BehaviorRule::new(
            "r-safe",
            "customer.retain",
            json!({ "target": "cust-01" }),
            ConditionExpr::Not(Box::new(ConditionExpr::HasRisk {
                action: "RefundCustomer".into(),
            })),
        ))
        .with_rule(BehaviorRule::new(
            "r-risk",
            "customer.refund",
            json!({ "target": "cust-01", "amount": 100 }),
            ConditionExpr::HasRisk {
                action: "RefundCustomer".into(),
            },
        ));
    let engine = RuleEngine::from_store(&store);
    let memory = Memory::new();
    let view = observe(&k, ctx, "goal", &memory, &caps_of(), Some(&ev));

    let d = engine.decide(&view).unwrap().expect("决策");
    assert_eq!(
        d.capability, "customer.refund",
        "HasRisk 条件触发风险回避路径"
    );
    assert_eq!(d.rule.as_deref(), Some("r-risk"));
}

// ── F3：同一路径 — RuleEngine 进 AgentLoop 全流程（审批零 bypass）─────
#[tokio::test]
async fn tc_f3_same_execution_path() {
    let services = [crm_service()];
    let mut k = Kernel::new();
    let mut agent = spawn_agent(&mut k);
    let ctx = agent.context;
    let mut stats = RuleStatsStore::new();

    // ① 免审批闭环：Not(active) → activate
    let store = RuleStore::new().with_rule(BehaviorRule::new(
        "r-activate",
        "customer.activate",
        json!({ "target": "u1" }),
        not_active(),
    ));
    let engine = RuleEngine::from_store(&store);
    let task = Task::new("activate", "激活").expecting(&[("status", "active")]);
    let out = {
        let mut loop_ =
            AgentLoop::new(&mut k, &mut agent, &services, &engine).with_rule_stats(&mut stats);
        loop_.run_until_done(ctx, &task, json!({})).unwrap()
    };
    assert_eq!(out, LoopOutcome::Completed);
    assert_eq!(
        stats.stats_of("r-activate").map(|s| s.success),
        Some(1),
        "规则被选中且走标准路径"
    );

    // ② 审批路径：status→active 后 → refund（require_approval）→ Waiting → 人工 → 完成
    let store2 = RuleStore::new().with_rule(BehaviorRule::new(
        "r-refund",
        "customer.refund",
        json!({ "target": "u1", "amount": 100 }),
        ConditionExpr::All(vec![
            ConditionExpr::HasRelation {
                predicate: "status".into(),
                target: "active".into(),
            },
            // 防重：目标达成后停止（D5 目标函数语义，同 FakeEngine when 模式）
            ConditionExpr::Not(Box::new(ConditionExpr::HasRelation {
                predicate: "refunded".into(),
                target: "true".into(),
            })),
        ]),
    ));
    let engine2 = RuleEngine::from_store(&store2);
    let task2 = Task::new("refund", "退款").expecting(&[("refunded", "true")]);
    let out2 = {
        let mut loop_ =
            AgentLoop::new(&mut k, &mut agent, &services, &engine2).with_rule_stats(&mut stats);
        loop_.run_until_done(ctx, &task2, json!({})).unwrap()
    };
    assert_eq!(out2, LoopOutcome::Waiting, "审批不 bypass");
    let fiber = k
        .all_fibers()
        .find(|f| f.context == ctx && f.task == "refund")
        .unwrap()
        .id;
    k.approve(fiber, "admin").unwrap();
    let out3 = {
        let mut loop_ =
            AgentLoop::new(&mut k, &mut agent, &services, &engine2).with_rule_stats(&mut stats);
        loop_.resume_until_done(fiber, &task2).unwrap()
    };
    assert_eq!(out3, LoopOutcome::Completed);
    assert_eq!(stats.stats_of("r-refund").map(|s| s.success), Some(1));
    // Verify 走 kernel：世界真实达成
    assert!(k.relations().any(|r| r.predicate == "refunded"));
}

// ── F4：双尺度经验（rule 级与 action 级并存）──────────────────────────
#[tokio::test]
async fn tc_f4_double_scale_stats() {
    let services = [crm_service()];
    let mut k = Kernel::new();
    let mut stats = RuleStatsStore::new();
    let mut ev = EvolutionEngine::new();

    for i in 1..=3 {
        let target = format!("u{i}");
        // 独立 context：Not(active) 按作用域隔离，每轮条件为真 → 3 次都执行
        let ctx_i = k.create_context(None, Scope::World, "w").unwrap();
        let mut agent_i = Agent::new(format!("agent-{i}"), ctx_i);
        let engine = RuleEngine::from_store(&RuleStore::new().with_rule(BehaviorRule::new(
            "r-activate",
            "customer.activate",
            json!({ "target": target.clone() }),
            ConditionExpr::Not(Box::new(ConditionExpr::HasRelation {
                predicate: "status".into(),
                target: "active".into(),
            })),
        )));
        let task = Task::new(format!("activate-{i}"), "激活").expecting(&[("status", "active")]);
        let out = {
            let mut loop_ = AgentLoop::new(&mut k, &mut agent_i, &services, &engine)
                .with_rule_stats(&mut stats)
                .with_evolution(&mut ev);
            loop_.run_until_done(ctx_i, &task, json!({})).unwrap()
        };
        assert_eq!(out, LoopOutcome::Completed);
    }
    // 规则级
    assert_eq!(stats.stats_of("r-activate").map(|s| s.success), Some(3));
    // action 级（Phase 6 并存）
    assert_eq!(
        ev.experience("ActivateCustomer").map(|e| e.success),
        Some(3)
    );
}

// ── F5：提案提炼（高频失败 → Disable；高频成功 → Promote）─────────────
#[tokio::test]
async fn tc_f5_proposal_mining() {
    let r_bad = BehaviorRule::new(
        "r-bad",
        "customer.refund",
        json!({ "target": "u1", "amount": 2000 }),
        ConditionExpr::HasRelation {
            predicate: "status".into(),
            target: "active".into(),
        },
    );
    let r_good = BehaviorRule::new(
        "r-good",
        "customer.retain",
        json!({ "target": "u2" }),
        ConditionExpr::HasRelation {
            predicate: "status".into(),
            target: "active".into(),
        },
    );
    let mut stats = RuleStatsStore::new();
    for _ in 0..3 {
        stats.record("r-bad", OutcomeKind::Failed);
    }
    stats.record("r-bad", OutcomeKind::Compensated);
    stats.record("r-bad", OutcomeKind::Success); // 样本 5，失败信号 4，占比 0.8
    for _ in 0..5 {
        stats.record("r-good", OutcomeKind::Success);
    }

    let proposals = propose_changes(&[r_bad.clone(), r_good.clone()], &stats);
    let disable = proposals
        .iter()
        .find(|p| matches!(p.change, BehaviorChange::Disable { .. }))
        .expect("Disable 提案");
    assert_eq!(disable.target_rule_id, "r-bad");
    assert_eq!(disable.expected_version, 1);
    assert_eq!(disable.evidence.confidence, 0.8, "失败占比");
    let promote = proposals
        .iter()
        .find(|p| matches!(p.change, BehaviorChange::Promote { .. }))
        .expect("Promote 提案");
    assert_eq!(promote.target_rule_id, "r-good");
    assert_eq!(promote.evidence.confidence, 1.0, "成功置信");
    assert!(matches!(
        promote.change,
        BehaviorChange::Promote {
            new_priority: 10,
            ..
        }
    ));
}

// ── F6：Validation 门槛（样本/置信不足 → 不 adopt）────────────────────
#[tokio::test]
async fn tc_f6_validation_gates() {
    // 样本不足（total=2 < 5）——手工构造弱提案（提炼门槛外，直接测治理闸门）
    let r = BehaviorRule::new(
        "r1",
        "customer.retain",
        json!({}),
        ConditionExpr::HasRelation {
            predicate: "status".into(),
            target: "active".into(),
        },
    );
    let ctx = validate_ctx(vec![r.clone()]);
    let weak = BehaviorProposal {
        id: "bp.disable.r1".into(),
        target_rule_id: "r1".into(),
        expected_version: 1,
        expected_snapshot_hash: 0,
        change: BehaviorChange::Disable {
            rule_id: "r1".into(),
        },
        evidence: openaip_os::rule::ProposalEvidence {
            failed: 2,
            confidence: 1.0,
            ..Default::default()
        },
        checks: Vec::new(),
        status: ValidationStatus::Proposed,
    };
    assert_eq!(
        validate(&weak, &ctx),
        ValidationStatus::Rejected,
        "样本不足"
    );

    // 置信不足（失败占比 0.4 < 0.8）
    let low_conf = BehaviorProposal {
        id: "bp.disable.r2".into(),
        target_rule_id: "r1".into(),
        expected_version: 1,
        expected_snapshot_hash: 0,
        change: BehaviorChange::Disable {
            rule_id: "r1".into(),
        },
        evidence: openaip_os::rule::ProposalEvidence {
            failed: 2,
            success: 3,
            confidence: 0.4,
            ..Default::default()
        },
        checks: Vec::new(),
        status: ValidationStatus::Proposed,
    };
    assert_eq!(
        validate(&low_conf, &ctx),
        ValidationStatus::Rejected,
        "置信不足"
    );
}

// ── F7：Adoption 后行为改变 ───────────────────────────────────────────
#[tokio::test]
async fn tc_f7_adoption_changes_behavior() {
    let mut k = Kernel::new();
    let ctx = k.create_context(None, Scope::World, "w").unwrap();
    let t0 = k.now();
    k.relate(Relation::new(ctx, "u1", "status", "active", t0))
        .unwrap();

    let r_a = BehaviorRule::new(
        "r-a",
        "customer.activate",
        json!({ "target": "u1" }),
        ConditionExpr::HasRelation {
            predicate: "status".into(),
            target: "active".into(),
        },
    )
    .with_priority(10);
    let r_b = BehaviorRule::new(
        "r-b",
        "customer.refund",
        json!({ "target": "u1", "amount": 100 }),
        ConditionExpr::HasRelation {
            predicate: "status".into(),
            target: "active".into(),
        },
    )
    .with_priority(20);

    // 初始：r-b 优先（priority 20 > 10）
    let engine = RuleEngine::new(vec![r_a.clone(), r_b.clone()]);
    let view = observe(&k, ctx, "goal", &Memory::new(), &caps_of(), None);
    let d = engine.decide(&view).unwrap().unwrap();
    assert_eq!(d.rule.as_deref(), Some("r-b"));

    // Promote r-a → 30 → 行为改变：r-a 优先
    let mut evolver = RuleEvolver::new();
    let promote = BehaviorProposal {
        id: "bp.promote.r-a".into(),
        target_rule_id: "r-a".into(),
        expected_version: r_a.version,
        expected_snapshot_hash: r_a.snapshot_hash(),
        change: BehaviorChange::Promote {
            rule_id: "r-a".into(),
            new_priority: 30,
        },
        evidence: Default::default(),
        checks: Vec::new(),
        status: ValidationStatus::Proposed,
    };
    // 证据填满（样本 5、置信 1.0）
    let mut promote = promote;
    promote.evidence.success = 5;
    promote.evidence.confidence = 1.0;
    let mut rules = vec![r_a.clone(), r_b.clone()];
    let ctxv = validate_ctx(rules.clone());
    evolver.adopt(&promote, &mut rules, &ctxv).unwrap();
    assert_eq!(rules[0].priority, 30);
    assert_eq!(rules[0].version, 2);

    let engine2 = RuleEngine::new(rules.clone());
    let d2 = engine2.decide(&view).unwrap().unwrap();
    assert_eq!(d2.rule.as_deref(), Some("r-a"), "Promote 后 r-a 优先");

    // Disable r-b → 行为改变：不再选 r-b
    let disable = BehaviorProposal {
        id: "bp.disable.r-b".into(),
        target_rule_id: "r-b".into(),
        expected_version: 1,
        expected_snapshot_hash: r_b.snapshot_hash(),
        change: BehaviorChange::Disable {
            rule_id: "r-b".into(),
        },
        evidence: Default::default(),
        checks: Vec::new(),
        status: ValidationStatus::Proposed,
    };
    let mut disable = disable;
    disable.evidence.failed = 3;
    disable.evidence.compensated = 2;
    disable.evidence.confidence = 1.0;
    evolver.adopt(&disable, &mut rules, &ctxv).unwrap();
    assert!(!rules[1].enabled);

    let engine3 = RuleEngine::new(rules.clone());
    let d3 = engine3.decide(&view).unwrap().unwrap();
    assert_eq!(d3.rule.as_deref(), Some("r-a"), "r-b 停用后只走 r-a");
}

// ── F8：端到端 — 经验 → 提案 → 治理验证 → adopt → 行为改变 → 审计 → 撤销 ─
#[tokio::test]
async fn tc_f8_end_to_end_behavior_evolution() {
    let r_a = BehaviorRule::new(
        "r-a",
        "customer.activate",
        json!({ "target": "u1" }),
        ConditionExpr::HasRelation {
            predicate: "status".into(),
            target: "active".into(),
        },
    )
    .with_priority(10);
    let r_b = BehaviorRule::new(
        "r-b",
        "customer.refund",
        json!({ "target": "u1", "amount": 100 }),
        ConditionExpr::HasRelation {
            predicate: "status".into(),
            target: "active".into(),
        },
    )
    .with_priority(20);

    // 经验：r-b 高频失败（认知风险结构化）
    let mut stats = RuleStatsStore::new();
    for _ in 0..3 {
        stats.record("r-b", OutcomeKind::Failed);
    }
    stats.record("r-b", OutcomeKind::Compensated);
    stats.record("r-b", OutcomeKind::Success);

    // 提炼 → 治理验证（fallback = r-a 仍在）
    let mut rules = vec![r_a.clone(), r_b.clone()];
    let proposals = propose_changes(&rules, &stats);
    let disable = proposals
        .iter()
        .find(|p| matches!(p.change, BehaviorChange::Disable { .. }))
        .expect("Disable 提案");
    let ctxv = validate_ctx(rules.clone());
    assert_eq!(validate(disable, &ctxv), ValidationStatus::Validated);

    // adopt（无人工审批；Validation 即治理闸门）
    let mut evolver = RuleEvolver::new();
    evolver.adopt(disable, &mut rules, &ctxv).unwrap();
    assert!(!rules[1].enabled, "r-b 已停用");
    assert_eq!(rules[1].version, 2);
    assert_eq!(evolver.log().len(), 1, "adoption_log 审计");

    // 行为改变：新引擎不再选 r-b
    let mut k = Kernel::new();
    let ctx = k.create_context(None, Scope::World, "w").unwrap();
    let t0 = k.now();
    k.relate(Relation::new(ctx, "u1", "status", "active", t0))
        .unwrap();
    let view = observe(&k, ctx, "goal", &Memory::new(), &caps_of(), None);
    let engine = RuleEngine::new(rules.clone());
    let d = engine.decide(&view).unwrap().unwrap();
    assert_eq!(d.rule.as_deref(), Some("r-a"), "高频失败规则被停用");

    // 撤销恢复：undo → r-b 恢复启用（version 回退）
    evolver.undo(&mut rules, &disable.id).unwrap();
    assert!(rules[1].enabled, "undo 后恢复");
    assert_eq!(rules[1].version, 1, "版本回退");
    let engine2 = RuleEngine::new(rules.clone());
    let d2 = engine2.decide(&view).unwrap().unwrap();
    assert_eq!(d2.rule.as_deref(), Some("r-b"), "恢复后 r-b 重新优先");
}

// ── F-Rev1：无后门（未 Validated 无法 adopt；行为变化必有审计）────────
#[tokio::test]
async fn tc_rev1_no_backdoor() {
    let r = BehaviorRule::new(
        "r1",
        "customer.activate",
        json!({ "target": "u1" }),
        ConditionExpr::HasRelation {
            predicate: "status".into(),
            target: "active".into(),
        },
    );
    let ctxv = validate_ctx(vec![r.clone()]);

    // 证据不足的提案（未过治理验证）
    let weak = BehaviorProposal {
        id: "bp.disable.r1".into(),
        target_rule_id: "r1".into(),
        expected_version: 1,
        expected_snapshot_hash: 0,
        change: BehaviorChange::Disable {
            rule_id: "r1".into(),
        },
        evidence: Default::default(), // 样本 0 < 5
        checks: Vec::new(),
        status: ValidationStatus::Proposed,
    };
    let mut rules = vec![r];
    let mut evolver = RuleEvolver::new();
    assert!(
        matches!(
            evolver.adopt(&weak, &mut rules, &ctxv),
            Err(RuleError::NotValidated(_))
        ),
        "未 Validated 无法 adopt（无后门）"
    );
    assert!(rules[0].enabled, "规则未被修改");
    assert!(evolver.log().is_empty(), "无审计 = 无变更");
}

// ── F-Rev2：不侵 World（adopt 全程 kernel 零异常）─────────────────────
#[tokio::test]
async fn tc_rev2_never_touches_world() {
    let mut k = Kernel::new();
    let ctx = k.create_context(None, Scope::World, "w").unwrap();
    k.relate(Relation::new(ctx, "u1", "status", "active", k.now()))
        .unwrap();

    let events_before = k.events().len();
    let relations_before = k.relations().count();

    let r = BehaviorRule::new(
        "r1",
        "customer.activate",
        json!({ "target": "u1" }),
        ConditionExpr::HasRelation {
            predicate: "status".into(),
            target: "active".into(),
        },
    );
    let ctxv = validate_ctx(vec![r.clone()]);
    let mut rules = vec![r.clone()];
    let mut evolver = RuleEvolver::new();
    let p = BehaviorProposal {
        id: "bp.promote.r1".into(),
        target_rule_id: "r1".into(),
        expected_version: 1,
        expected_snapshot_hash: r.snapshot_hash(),
        change: BehaviorChange::Promote {
            rule_id: "r1".into(),
            new_priority: 99,
        },
        evidence: {
            openaip_os::rule::ProposalEvidence {
                success: 5,
                confidence: 1.0,
                ..Default::default()
            }
        },
        checks: Vec::new(),
        status: ValidationStatus::Proposed,
    };
    evolver.adopt(&p, &mut rules, &ctxv).unwrap();

    assert_eq!(
        k.events().len(),
        events_before,
        "adopt 不产生 kernel 事件（不侵 World）"
    );
    assert_eq!(
        k.relations().count(),
        relations_before,
        "adopt 不产生关系（不侵 World）"
    );
}

// ── F-Rev3：no-dead-agent（Disable 最后一个可行规则 → reject）─────────
#[tokio::test]
async fn tc_rev3_no_dead_agent() {
    let r = BehaviorRule::new(
        "r1",
        "customer.activate",
        json!({ "target": "u1" }),
        ConditionExpr::HasRelation {
            predicate: "status".into(),
            target: "active".into(),
        },
    );
    let mut stats = RuleStatsStore::new();
    for _ in 0..3 {
        stats.record("r1", OutcomeKind::Failed);
    }
    stats.record("r1", OutcomeKind::Compensated);
    stats.record("r1", OutcomeKind::Success);

    let proposals = propose_changes(std::slice::from_ref(&r), &stats);
    let disable = proposals[0].clone();
    let ctxv = validate_ctx(vec![r.clone()]);
    assert_eq!(
        validate(&disable, &ctxv),
        ValidationStatus::Rejected,
        "Disable 最后一个可行规则 → reject（no-dead-agent）"
    );
    // adopt 也被拒（治理闸门一致）
    let mut rules = vec![r];
    let mut evolver = RuleEvolver::new();
    assert!(matches!(
        evolver.adopt(&disable, &mut rules, &ctxv),
        Err(RuleError::NotValidated(_))
    ));
    assert!(rules[0].enabled);
}

// ── F-Rev4：stale proposal（v3 提案 → 规则变 v4 → adopt reject）───────
#[tokio::test]
async fn tc_rev4_stale_proposal_rejected() {
    let r = BehaviorRule::new(
        "r1",
        "customer.activate",
        json!({ "target": "u1" }),
        ConditionExpr::HasRelation {
            predicate: "status".into(),
            target: "active".into(),
        },
    );
    let ctxv = validate_ctx(vec![r.clone()]);
    let mut rules = vec![r.clone()];
    let mut evolver = RuleEvolver::new();

    // 第一个提案：Promote（expected_version 1）→ 成功 → 规则变 v2
    let p1 = BehaviorProposal {
        id: "bp.promote.r1".into(),
        target_rule_id: "r1".into(),
        expected_version: 1,
        expected_snapshot_hash: r.snapshot_hash(),
        change: BehaviorChange::Promote {
            rule_id: "r1".into(),
            new_priority: 50,
        },
        evidence: {
            openaip_os::rule::ProposalEvidence {
                success: 5,
                confidence: 1.0,
                ..Default::default()
            }
        },
        checks: Vec::new(),
        status: ValidationStatus::Proposed,
    };
    evolver.adopt(&p1, &mut rules, &ctxv).unwrap();
    assert_eq!(rules[0].version, 2);

    // 第二个提案：仍以 expected_version 1 为目标（过期）
    let p2 = BehaviorProposal {
        id: "bp.disable.r1".into(),
        target_rule_id: "r1".into(),
        expected_version: 1,
        expected_snapshot_hash: 0,
        change: BehaviorChange::Disable {
            rule_id: "r1".into(),
        },
        evidence: {
            openaip_os::rule::ProposalEvidence {
                failed: 3,
                compensated: 2,
                confidence: 1.0,
                ..Default::default()
            }
        },
        checks: Vec::new(),
        status: ValidationStatus::Proposed,
    };
    assert!(
        matches!(
            evolver.adopt(&p2, &mut rules, &ctxv),
            Err(RuleError::Stale(_, 2, 1))
        ),
        "旧提案不能覆盖新行为状态（stale）"
    );
    assert!(rules[0].enabled, "规则未被过期提案修改");
}
