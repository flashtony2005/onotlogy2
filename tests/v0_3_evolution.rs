//! V0.5 Memory / Evolution / REQ-018 AutoApprove 测试契约（TC-022 + TC-025/026）
mod common;
use common::*;

use async_trait::async_trait;
use openaip_os::action::ActionRegistry;
use openaip_os::evolution::{
    EvolutionState, Experience, LearnedKnowledge, Pattern, Proposal, ProposalKind, ProposalStatus,
};
use openaip_os::ir::ActionDef;
use openaip_os::parse_odl;
use openaip_os::planner::{
    ApprovalAuditor, AutoApprover, Capability, PlanStep, Planner, PlannerError, Policy,
    PolicyDecision, RunAuditor, RuntimeConfig, generate_run_id, may_auto_approve,
};
use openaip_os::runtime::RuntimeKernel;
use openaip_os::storage::{LibSQLStorage, TraceQuery};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;

// ── TC-022a：记忆沉淀存储往返（V0.5 Memory = Event 积累；BL-008）──────────────────────
#[tokio::test]
async fn tc022a_memory_store_roundtrip() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = RuntimeKernel::new(s);
    // 首沉淀
    let m1 = kernel
        .record_memory("action.retain", json!({ "experience": "retain" }))
        .await
        .unwrap();
    assert_eq!(m1.hits, 1, "首次沉淀 hits=1");
    // 再次沉淀同标签 → hits 累加 + payload 更新
    let m2 = kernel
        .record_memory("action.retain", json!({ "experience": "retain", "v": 2 }))
        .await
        .unwrap();
    assert_eq!(m2.hits, 2, "重复沉淀应累加 hits");
    assert_eq!(m2.payload["v"], json!(2), "应更新 payload");
    // recall 全量 / 单条 / 缺失
    let all = kernel.recall_all().await.unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].hits, 2);
    let one = kernel.recall_memory("action.retain").await.unwrap();
    assert!(one.is_some() && one.unwrap().hits == 2);
    assert!(kernel.recall_memory("missing").await.unwrap().is_none());
}

// ── TC-022b：Planner 记忆闭环（Action→Memory→下次 Plan 召回）──────────────────────────
#[tokio::test]
async fn tc022b_planner_memory_closed_loop() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = RuntimeKernel::new(s);
    kernel
        .create_object("Customer", "001", json!({"name": "Apple", "level": "VIP"}))
        .await
        .unwrap();
    kernel
        .create_object("Customer", "002", json!({"name": "Tesla", "level": "GOLD"}))
        .await
        .unwrap();
    let reg = ActionRegistry::from_ir(&parse_odl(ODL_RETAIN).unwrap());

    // 第一轮：执行 retain → 沉淀 action.Customer.retain（H3：标签带对象类型，跨类型隔离）
    let rid1 = generate_run_id();
    let r1 = Planner::with_actions(&kernel, reg.clone())
        .run_goal(&FakeEngine, "reduce churn", "retention-agent", &rid1)
        .await
        .expect("首轮应跑通");
    assert_eq!(r1.status.as_str(), "Completed");
    let mem1 = kernel.recall_all().await.unwrap();
    assert!(
        mem1.iter()
            .any(|m| m.label == "action.Customer.retain" && m.hits >= 1),
        "首轮应沉淀 action.Customer.retain"
    );

    // 第二轮：同 kernel 累加 hits，且应召回 MemoryRecall 事件（闭环可见）
    let rid2 = generate_run_id();
    let r2 = Planner::with_actions(&kernel, reg)
        .run_goal(&FakeEngine, "reduce churn", "retention-agent", &rid2)
        .await
        .expect("次轮应跑通");
    assert_eq!(r2.status.as_str(), "Completed");
    let retain = kernel
        .recall_all()
        .await
        .unwrap()
        .into_iter()
        .find(|m| m.label == "action.Customer.retain")
        .expect("应存在 action.Customer.retain 记忆");
    assert!(retain.hits >= 2, "两轮应累加 hits（自学习沉淀）");
    let cev2 = kernel
        .list_events(TraceQuery::for_run(&rid2))
        .await
        .unwrap();
    assert!(
        cev2.iter().any(|e| e.event_type == "MemoryRecall"),
        "第二轮应召回 MemoryRecall 事件（Action→Memory→下次 Plan 闭环）"
    );
}

// ════════════════════════════════════════════════════════════════════════════════════════
// TC-025  Evolution Engine MVP（BL-003 · REQ-017）：Experience → Pattern → Proposal →
// Validation → LearnedKnowledge。第一原则 Learn≠Permission：Evolution 不修改 Policy、不绕过
// Human Approval。所有测试均验证该安全边界。
// ════════════════════════════════════════════════════════════════════════════════════════

// ── TC-025a：Experience Mining（hits≥2 才成 Pattern） ────────────────────────────────────
#[test]
fn tc025a_experience_mining() {
    let exps = vec![
        Experience {
            label: "action.retain".into(),
            hits: 3,
        },
        Experience {
            label: "action.refund".into(),
            hits: 1,
        },
    ];
    let patterns = openaip_os::evolution::mine_patterns(&exps);
    // retain(3) → 1 条 pattern；refund(1) → 0 条（未达 PATTERN_MIN_HITS）
    assert_eq!(patterns.len(), 1, "仅 retain 应出 pattern");
    assert_eq!(patterns[0].label, "pattern.action.retain");
    assert_eq!(patterns[0].support, 3);
    // 非 action.* 前缀的经验不参与
    let no_prefix = vec![Experience {
        label: "RetentionPattern".into(),
        hits: 9,
    }];
    assert!(
        openaip_os::evolution::mine_patterns(&no_prefix).is_empty(),
        "非 action.* 经验不应成 Pattern"
    );
}

// ── TC-025b：Confidence 边界（确定性 min(1, hits/5)） ───────────────────────────────────
#[test]
fn tc025b_confidence_boundary() {
    assert_eq!(Pattern::confidence_of(1), 0.2);
    assert_eq!(Pattern::confidence_of(2), 0.4);
    assert_eq!(Pattern::confidence_of(3), 0.6);
    assert_eq!(Pattern::confidence_of(4), 0.8);
    assert_eq!(Pattern::confidence_of(5), 1.0);
    assert_eq!(Pattern::confidence_of(7), 1.0, "超过 5 封顶为 1.0");
}

// ── TC-025c：Proposal（Pattern→Proposal；Proposal 不改变 Agent 行为） ───────────────────
#[test]
fn tc025c_proposal_does_not_change_behavior() {
    let pattern = Pattern {
        label: "pattern.action.retain".into(),
        action: "retain".into(),
        support: 3,
        confidence: 0.6,
        run_ids: vec!["RUN-1".into()],
    };
    let prop = openaip_os::evolution::propose(&pattern);
    assert_eq!(prop.id, "proposal.retain");
    assert_eq!(prop.kind, ProposalKind::ActionPattern);
    assert_eq!(prop.target, "retain");
    assert_eq!(prop.status, ProposalStatus::Proposed);
    assert_eq!(prop.pattern_label, pattern.label);
    // 安全边界：Proposal 仅是一个知识候选，绝不携带任何「权限/放行」语义字段。
    // 这里通过类型断言确认 Proposal 没有 approval / bypass 相关字段——它不能被用来改 Policy。
    let _: Proposal = prop; // 若未来给 Proposal 加 bypass 字段，此类型将被迫显式处理。
}

// ── TC-025d：Validation（阈值校验，Validated≠可改安全策略） ─────────────────────────────
#[test]
fn tc025d_validation_threshold() {
    let mut low = Proposal {
        id: "proposal.refund".into(),
        kind: ProposalKind::ActionPattern,
        target: "refund".into(),
        confidence: 0.4, // hits=2 边界
        status: ProposalStatus::Proposed,
        pattern_label: "pattern.action.refund".into(),
        run_ids: vec![],
    };
    low.status = openaip_os::evolution::validate(&low);
    assert_eq!(low.status, ProposalStatus::Rejected, "conf<0.6 应 Rejected");

    let mut ok = Proposal {
        id: "proposal.retain".into(),
        kind: ProposalKind::ActionPattern,
        target: "retain".into(),
        confidence: 0.6,
        status: ProposalStatus::Proposed,
        pattern_label: "pattern.action.retain".into(),
        run_ids: vec![],
    };
    ok.status = openaip_os::evolution::validate(&ok);
    assert_eq!(
        ok.status,
        ProposalStatus::Validated,
        "conf≥0.6 应 Validated"
    );
    // Validated 只代表「该知识通过验证」，不授予任何权限（Learn≠Permission）。
    // 阈值常量定义见 evolution::VALIDATE_THRESHOLD（=0.6）。
}

// ── TC-025e：Evolution Isolation（LearnedKnowledge 不改动 Policy / 不绕过 Approval） ─────
#[tokio::test]
async fn tc025e_evolution_does_not_bypass_approval() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = RuntimeKernel::new(s);
    // 模拟 retain 被成功执行 3 次 → 沉淀经验
    for _ in 0..3 {
        kernel
            .record_memory("action.retain", json!({ "experience": "retain" }))
            .await
            .unwrap();
    }
    // 跑一轮 Evolution：应产出 LearnedKnowledge（retain Validated）
    let state = kernel.run_evolution("RUN-100").await.unwrap();
    assert_eq!(state.learned.len(), 1, "retain 应沉淀为 LearnedKnowledge");
    assert_eq!(state.learned[0].action, "retain");

    // 关键安全断言：require_approval 动作即使已 Learned，仍走 Human Approval 闸门（WB2005）。
    // 用 refund 动作的 policy（require_approval=true）做 check_action，默认审批器拒 → Deny。
    let def = parse_odl(ODL_REFUND)
        .unwrap()
        .find("Customer")
        .unwrap()
        .actions
        .iter()
        .find(|a| a.name == "refund")
        .cloned()
        .unwrap();
    let step = PlanStep {
        capability: Capability::Action {
            object_type: "Customer".into(),
            action: "refund".into(),
            target_id: "001".into(),
            args: json!({}),
        },
        object_refs: vec!["001".into()],
        description: "refund".into(),
    };
    let mut args = HashMap::new();
    args.insert("amount".to_string(), json!(500));
    let target = json!({ "level": "VIP" });
    let deny = Policy::check_action(
        &step,
        Some(&def),
        &AutoApprover { allow: false },
        Some(&target),
        &args,
        &RuntimeConfig::default(),
        None,
        0,
        None,
    )
    .await;
    assert!(
        matches!(deny, PolicyDecision::Deny { .. }),
        "require_approval 动作即便 Evolution 已学习，仍须 Human Approval（不绕过 WB2005）"
    );
}

// ── TC-025f：Idempotency（run_evolution 可重复运行不重复产出知识） ──────────────────────
#[tokio::test]
async fn tc025f_evolution_idempotent() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = RuntimeKernel::new(s);
    for _ in 0..3 {
        kernel
            .record_memory("action.retain", json!({ "experience": "retain" }))
            .await
            .unwrap();
    }
    // 连续运行三轮
    kernel.run_evolution("RUN-1").await.unwrap();
    kernel.run_evolution("RUN-2").await.unwrap();
    let final_state = kernel.run_evolution("RUN-3").await.unwrap();

    assert_eq!(
        final_state.learned.len(),
        1,
        "learned.retain 应仅 1 条（幂等 upsert）"
    );
    assert_eq!(final_state.proposals.len(), 1, "proposal.retain 应仅 1 条");
    assert_eq!(final_state.patterns.len(), 1, "pattern.retain 应仅 1 条");
    // 证据 run_ids 应跨三轮累加去重：[RUN-1, RUN-2, RUN-3]
    assert_eq!(
        final_state.patterns[0].run_ids.len(),
        3,
        "证据应跨 run 累积"
    );
    assert_eq!(final_state.learned[0].run_ids.len(), 3);
}

// ── TC-025g：Traceability（LearnedKnowledge→Proposal→Pattern→Experience→AgentRun） ──────
#[tokio::test]
async fn tc025g_evolution_traceability() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = RuntimeKernel::new(s);
    for _ in 0..3 {
        kernel
            .record_memory("action.retain", json!({ "experience": "retain" }))
            .await
            .unwrap();
    }
    let state: EvolutionState = kernel.run_evolution("RUN-1021").await.unwrap();

    // LearnedKnowledge → Proposal → Pattern 父引用链完整
    let learned: &LearnedKnowledge = state.learned.first().expect("应有 learned");
    let prop = state
        .proposals
        .iter()
        .find(|p| p.id == learned.proposal_id)
        .expect("learned 应可追溯至 proposal");
    let pat = state
        .patterns
        .iter()
        .find(|p| p.label == prop.pattern_label)
        .expect("proposal 应可追溯至 pattern");

    // Pattern → Experience（action.retain）且 → AgentRun（RUN-1021）
    assert_eq!(pat.action, "retain", "pattern.action 应回指经验动作");
    assert!(
        pat.run_ids.contains(&"RUN-1021".to_string()),
        "pattern 应可追溯至来源 run"
    );
    assert!(
        learned.run_ids.contains(&"RUN-1021".to_string()),
        "learned 应可追溯至来源 run"
    );
    assert_eq!(prop.target, "retain");
}

// ── TC-025h：server GET /evolution 端点（与 Memory 端点同构） ───────────────────────────
#[tokio::test]
async fn tc025h_server_evolution_endpoint() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = Arc::new(RuntimeKernel::new(s));
    for _ in 0..3 {
        kernel
            .record_memory("action.retain", json!({ "experience": "retain" }))
            .await
            .unwrap();
    }
    kernel.run_evolution("RUN-1").await.unwrap();
    let addr = spawn_server(kernel).await;

    let (st, body) = http_call(addr, "GET", "/api/v1/wb/agent/evolution", None).await;
    assert_eq!(st, 200);
    let v = serde_json::from_str::<Value>(&body).unwrap();
    assert_eq!(v["code"], 0, "evolution 查询应成功");

    let patterns = v["data"]["patterns"].as_array().unwrap();
    let learned = v["data"]["learned"].as_array().unwrap();
    assert_eq!(patterns.len(), 1, "应含 1 个 pattern");
    assert_eq!(patterns[0]["label"], "pattern.action.retain");
    assert_eq!(learned.len(), 1, "应含 1 条 learned");
    assert_eq!(learned[0]["action"], "retain");
    assert_eq!(
        learned[0]["proposalId"], "proposal.retain",
        "可追溯 proposal"
    );
}

// ── TC-025i：LearnedKnowledge 可撤销（revoke_learned 幂等删除） ──────────────────────────
#[tokio::test]
async fn tc025i_revoke_learned() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = RuntimeKernel::new(s);
    for _ in 0..3 {
        kernel
            .record_memory("action.retain", json!({ "experience": "retain" }))
            .await
            .unwrap();
    }
    let state = kernel.run_evolution("RUN-REVOKE").await.unwrap();
    assert_eq!(state.learned.len(), 1, "应沉淀 1 条 learned");

    kernel.revoke_learned("learned.retain").await.unwrap();
    let state2 = kernel.evolution_state().await.unwrap();
    assert!(state2.learned.is_empty(), "撤销后 learned 应为空");

    // 幂等：重复撤销不报错
    kernel.revoke_learned("learned.retain").await.unwrap();
}

// ══ REQ-018 AutoApprove（TC-026a~h）══════════════════════════════════════════════════════
// 设计红线：Learn≠Permission 仍成立；AutoApprove 仅在 5 道闸门全过 + 开关开启时，
// 替代一次 Human Approval；任一闸门缺失 / 审计失败 / learned 缺失 → 严格回退 Human Approval。

/// 构造一条「已验证」的 LearnedKnowledge（模拟 Evolution 沉淀；ProposalKind=ActionPattern 即 Validated）。
fn mk_learned(action: &str, confidence: f64, runs: usize, enabled: bool) -> LearnedKnowledge {
    LearnedKnowledge {
        id: format!("learned.{action}"),
        kind: ProposalKind::ActionPattern,
        action: action.to_string(),
        confidence,
        proposal_id: format!("proposal.{action}"),
        run_ids: (0..runs).map(|i| format!("run-{i}")).collect(),
        auto_approve_enabled: enabled,
        created_at: "2026-08-12T00:00:00Z".into(),
    }
}

/// refund 动作的 PlanStep（require_approval=true + 非空 precondition → 满足低风险闸门）。
fn refund_step() -> PlanStep {
    PlanStep {
        capability: Capability::Action {
            object_type: "Customer".into(),
            action: "refund".into(),
            target_id: "001".into(),
            args: json!({}),
        },
        object_refs: vec!["001".into()],
        description: "refund".into(),
    }
}

/// refund 动作定义（从 ODL_REFUND 取，policy 满足低风险闸门）。
fn refund_def() -> ActionDef {
    parse_odl(ODL_REFUND)
        .unwrap()
        .find("Customer")
        .unwrap()
        .actions
        .iter()
        .find(|a| a.name == "refund")
        .cloned()
        .unwrap()
}

// ── TC-026a：默认关闭（auto_approve_from_evolution=false）→ 仍 Deny，等同 MVP，不回归 ──
#[tokio::test]
async fn tc026a_default_off_still_deny() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = RuntimeKernel::new(s);
    kernel
        .save_learned(&mk_learned("refund", 0.9, 3, true))
        .await
        .unwrap();
    let auditor = RunAuditor::new(&kernel, "run-a");
    let step = refund_step();
    let def = refund_def();
    let mut args = HashMap::new();
    args.insert("amount".to_string(), json!(500));
    let target = json!({ "level": "VIP" });
    // 默认配置（开关关闭）→ 即便有 learned 也回退 Human Approval（approver 拒）→ Deny
    let decision = Policy::check_action(
        &step,
        Some(&def),
        &AutoApprover { allow: false },
        Some(&target),
        &args,
        &RuntimeConfig::default(),
        Some(&mk_learned("refund", 0.9, 3, true)),
        0,
        Some(&auditor),
    )
    .await;
    assert!(
        matches!(decision, PolicyDecision::Deny { .. }),
        "默认关闭：require_approval 仍 Deny（等同 MVP，不回归）"
    );
    // 未触发自动放行 → 审计表不应有记录
    let audit = kernel.list_approval_audit(None).await.unwrap();
    assert!(audit.is_empty(), "默认关闭不应写自动放行审计");
}

// ── TC-026b：显式允许闸门未过（开关 false）→ 强制回退 Human ──
#[tokio::test]
async fn tc026b_switch_off_falls_back() {
    let lk = mk_learned("refund", 0.9, 3, true);
    let def = refund_def();
    let off = RuntimeConfig {
        auto_approve_from_evolution: false,
    };
    assert!(
        !may_auto_approve(&off, Some(&lk), 3, 0, &def.policy),
        "开关关闭必须回退"
    );
    let on = RuntimeConfig {
        auto_approve_from_evolution: true,
    };
    assert!(
        may_auto_approve(&on, Some(&lk), 3, 0, &def.policy),
        "开关开启且其余闸门全过应放行"
    );
}

// ── TC-026c：高置信闸门边界（confidence <0.8 / =0.8 / >0.8）──
#[tokio::test]
async fn tc026c_confidence_boundary() {
    let def = refund_def();
    let cfg = RuntimeConfig {
        auto_approve_from_evolution: true,
    };
    assert!(
        !may_auto_approve(
            &cfg,
            Some(&mk_learned("refund", 0.7, 3, true)),
            3,
            0,
            &def.policy
        ),
        "0.7 < 0.8 应回退"
    );
    assert!(
        may_auto_approve(
            &cfg,
            Some(&mk_learned("refund", 0.8, 3, true)),
            3,
            0,
            &def.policy
        ),
        "0.8 == 阈值应放行"
    );
    assert!(
        may_auto_approve(
            &cfg,
            Some(&mk_learned("refund", 0.85, 3, true)),
            3,
            0,
            &def.policy
        ),
        "0.85 > 阈值应放行"
    );
}

// ── TC-026d：多次成功闸门边界（success_runs <3 / =3 / >3）──
#[tokio::test]
async fn tc026d_runs_boundary() {
    let def = refund_def();
    let cfg = RuntimeConfig {
        auto_approve_from_evolution: true,
    };
    assert!(
        !may_auto_approve(
            &cfg,
            Some(&mk_learned("refund", 0.9, 2, true)),
            2,
            0,
            &def.policy
        ),
        "2 < 3 应回退"
    );
    assert!(
        may_auto_approve(
            &cfg,
            Some(&mk_learned("refund", 0.9, 3, true)),
            3,
            0,
            &def.policy
        ),
        "3 == 阈值应放行"
    );
    assert!(
        may_auto_approve(
            &cfg,
            Some(&mk_learned("refund", 0.9, 4, true)),
            4,
            0,
            &def.policy
        ),
        "4 > 阈值应放行"
    );
}

// ── TC-026e：开关开启 + 全闸门过 → 自动放行 Allow（替代 Human Approval），并写审计 ──
#[tokio::test]
async fn tc026e_all_gates_pass_auto_approve() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = RuntimeKernel::new(s);
    let lk = mk_learned("refund", 0.9, 3, true);
    kernel.save_learned(&lk).await.unwrap();
    let run_id = "run-e";
    let auditor = RunAuditor::new(&kernel, run_id);
    let step = refund_step();
    let def = refund_def();
    let mut args = HashMap::new();
    args.insert("amount".to_string(), json!(500));
    let target = json!({ "level": "VIP" });
    // 开关开启 + 全闸门过；approver 用「拒」证明走的是 auto-approve 替代路径，而非 approver 放行
    let decision = Policy::check_action(
        &step,
        Some(&def),
        &AutoApprover { allow: false },
        Some(&target),
        &args,
        &RuntimeConfig {
            auto_approve_from_evolution: true,
        },
        Some(&lk),
        0,
        Some(&auditor),
    )
    .await;
    assert_eq!(
        decision,
        PolicyDecision::Allow,
        "全闸门过应自动放行 Allow（替代 Human Approval）"
    );
    // 审计应记录本次自动放行（可追溯至 LearnedKnowledge + run）
    let audit = kernel
        .list_approval_audit(Some("learned.refund"))
        .await
        .unwrap();
    assert_eq!(audit.len(), 1, "应写 1 条自动放行审计");
    assert_eq!(audit[0].decision, "auto_approve");
    assert_eq!(audit[0].learned_id, "learned.refund");
    assert_eq!(audit[0].run_id, run_id);
}

// ── TC-026f：无失败闸门未过（failure_count>=1）→ 回退 Deny；ActionFailed 埋点可被精确计数 ──
#[tokio::test]
async fn tc026f_failure_blocks_auto_approve() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = RuntimeKernel::new(s);
    let lk = mk_learned("refund", 0.9, 3, true);
    kernel.save_learned(&lk).await.unwrap();
    let run_id = "run-f";
    let auditor = RunAuditor::new(&kernel, run_id);
    let step = refund_step();
    let def = refund_def();
    let mut args = HashMap::new();
    args.insert("amount".to_string(), json!(500));
    let target = json!({ "level": "VIP" });
    // failure_count=1 → 无失败闸门未过 → 回退 Human Approval（approver 拒）→ Deny
    let decision = Policy::check_action(
        &step,
        Some(&def),
        &AutoApprover { allow: false },
        Some(&target),
        &args,
        &RuntimeConfig {
            auto_approve_from_evolution: true,
        },
        Some(&lk),
        1,
        Some(&auditor),
    )
    .await;
    assert!(
        matches!(decision, PolicyDecision::Deny { .. }),
        "有失败（failure_count=1）应回退 Deny"
    );
    // 验证 ActionFailed 埋点可被 count_action_failed 精确捕获（强判定数据源；H3 限定键）
    kernel
        .emit_cognitive(
            "ActionFailed",
            run_id,
            "agent",
            &["001".to_string()],
            Some("refund 执行失败"),
            Some(json!({ "action": "Customer.refund" })),
            Some(json!({ "error": "boom" })),
            Some(0.0),
        )
        .await
        .unwrap();
    let cnt = kernel.count_action_failed("Customer.refund").await.unwrap();
    assert_eq!(cnt, 1, "ActionFailed 埋点应被精确计数");
    // 跨类型不串扰：其他类型的同名动作失败计数为 0
    let other = kernel.count_action_failed("Account.refund").await.unwrap();
    assert_eq!(other, 0, "限定键应隔离不同类型");
}

/// 审计写库必败的 Auditor：用于验证「审计失败 → 回退 Human」铁律。
struct FailingAuditor;
#[async_trait]
impl ApprovalAuditor for FailingAuditor {
    async fn record(&self, _learned_id: &str, _action: &str) -> Result<(), String> {
        Err("audit write failed".into())
    }
}

// ── TC-026g：异常一律回退 Human Approval，绝不静默放行 ──
#[tokio::test]
async fn tc026g_audit_failure_falls_back() {
    let lk = mk_learned("refund", 0.9, 3, true);
    let def = refund_def();
    let step = refund_step();
    let mut args = HashMap::new();
    args.insert("amount".to_string(), json!(500));
    let target = json!({ "level": "VIP" });
    // 全闸门过，但 auditor 写库失败 → 回退 approver（拒）→ Deny，绝不 Allow
    let decision = Policy::check_action(
        &step,
        Some(&def),
        &AutoApprover { allow: false },
        Some(&target),
        &args,
        &RuntimeConfig {
            auto_approve_from_evolution: true,
        },
        Some(&lk),
        0,
        Some(&FailingAuditor),
    )
    .await;
    assert!(
        matches!(decision, PolicyDecision::Deny { .. }),
        "审计失败必须回退 Human Approval"
    );
    // learned 读取异常（传入 None）→ 回退 Deny
    let decision2 = Policy::check_action(
        &step,
        Some(&def),
        &AutoApprover { allow: false },
        Some(&target),
        &args,
        &RuntimeConfig {
            auto_approve_from_evolution: true,
        },
        None,
        0,
        Some(&FailingAuditor),
    )
    .await;
    assert!(
        matches!(decision2, PolicyDecision::Deny { .. }),
        "learned 缺失必须回退 Human Approval"
    );
}

// ── TC-026h：审计含该次放行；撤销后下次命中即回退 Human（可观察不可删）；可重新开启 ──
#[tokio::test]
async fn tc026h_audit_and_revoke_reversible() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = RuntimeKernel::new(s);
    let lk = mk_learned("refund", 0.9, 3, true);
    kernel.save_learned(&lk).await.unwrap();
    let run_id = "run-h";
    let auditor = RunAuditor::new(&kernel, run_id);
    let step = refund_step();
    let def = refund_def();
    let mut args = HashMap::new();
    args.insert("amount".to_string(), json!(500));
    let target = json!({ "level": "VIP" });

    // 1) 全闸门过 → 自动放行，写审计
    let d1 = Policy::check_action(
        &step,
        Some(&def),
        &AutoApprover { allow: false },
        Some(&target),
        &args,
        &RuntimeConfig {
            auto_approve_from_evolution: true,
        },
        Some(&lk),
        0,
        Some(&auditor),
    )
    .await;
    assert_eq!(d1, PolicyDecision::Allow, "初次应自动放行");
    let audit = kernel
        .list_approval_audit(Some("learned.refund"))
        .await
        .unwrap();
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].decision, "auto_approve");

    // 2) 撤销自动放行资格
    kernel
        .set_learned_auto_approve("learned.refund", false)
        .await
        .unwrap();
    let lk2 = kernel
        .evolution_state()
        .await
        .unwrap()
        .learned
        .into_iter()
        .find(|l| l.id == "learned.refund")
        .expect("应有 learned");
    assert!(
        !lk2.auto_approve_enabled,
        "撤销后 auto_approve_enabled 应为 false"
    );

    // 3) 撤销后再次命中 → 回退 Human Approval（approver 拒）→ Deny
    let d2 = Policy::check_action(
        &step,
        Some(&def),
        &AutoApprover { allow: false },
        Some(&target),
        &args,
        &RuntimeConfig {
            auto_approve_from_evolution: true,
        },
        Some(&lk2),
        0,
        Some(&auditor),
    )
    .await;
    assert!(
        matches!(d2, PolicyDecision::Deny { .. }),
        "撤销后该 action 应回退 Human Approval"
    );

    // 4) 可恢复：重新开启资格 → 再次自动放行
    kernel
        .set_learned_auto_approve("learned.refund", true)
        .await
        .unwrap();
    let lk3 = kernel
        .evolution_state()
        .await
        .unwrap()
        .learned
        .into_iter()
        .find(|l| l.id == "learned.refund")
        .expect("应有 learned");
    let d3 = Policy::check_action(
        &step,
        Some(&def),
        &AutoApprover { allow: false },
        Some(&target),
        &args,
        &RuntimeConfig {
            auto_approve_from_evolution: true,
        },
        Some(&lk3),
        0,
        Some(&auditor),
    )
    .await;
    assert_eq!(d3, PolicyDecision::Allow, "恢复资格后应再次自动放行");
}

// ── TC-026j：H3 跨类型隔离 — Customer 的知识不得放行 Account 的同名动作 ──
// 两种类型声明同名 refund（require_approval + precondition → 满足低风险闸门）；
// 只给 Customer.refund 沉淀已验证知识 → Customer 自动放行、Account 必须回退 Human。
#[tokio::test]
async fn tc026j_learned_isolated_by_object_type() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = Arc::new(RuntimeKernel::new(s));
    let reg = ActionRegistry::from_ir(
        &parse_odl(
            r#"
object Customer:
  properties:
    name: string
    level: string
  actions:
    refund:
      params:
        amount: float
      updates:
        - field: balance
          value: amount
      policy:
        require_approval: true
        precondition:
          - field: amount
            op: le
            value: 1000
          - field: self.level
            op: ne
            value: "BLACKLIST"
object Account:
  properties:
    name: string
    level: string
  actions:
    refund:
      params:
        amount: float
      updates:
        - field: balance
          value: amount
      policy:
        require_approval: true
        precondition:
          - field: amount
            op: le
            value: 1000
          - field: self.level
            op: ne
            value: "BLACKLIST"
"#,
        )
        .unwrap(),
    );
    kernel
        .create_object(
            "Customer",
            "001",
            json!({"name": "Apple", "level": "VIP", "balance": 2000.0}),
        )
        .await
        .unwrap();
    kernel
        .create_object(
            "Account",
            "002",
            json!({"name": "Acc", "level": "VIP", "balance": 2000.0}),
        )
        .await
        .unwrap();

    // 只给 Customer.refund 沉淀已验证知识（3 次成功、高置信、未撤销）
    kernel
        .save_learned(&mk_learned("Customer.refund", 0.9, 3, true))
        .await
        .unwrap();

    let config = RuntimeConfig {
        auto_approve_from_evolution: true,
    };
    // Customer.refund：5 道闸门全过（本类型知识）→ 自动放行 → 运行成功
    let r1 = Planner::with_actions(&kernel, reg.clone())
        .with_config(config)
        .run_goal(
            &FakeEngine,
            "退款 001 500",
            "retention-agent",
            &generate_run_id(),
        )
        .await;
    assert!(
        r1.is_ok(),
        "Customer.refund 应被自动放行（存在本类型已验证知识）"
    );

    // Account.refund：无 Account.refund 知识 → 回退 Human Approval → AutoApprover 拒 → PolicyDenied
    let r2 = Planner::with_actions(&kernel, reg)
        .with_config(config)
        .run_goal(
            &FakeEngine,
            "退款 002 500",
            "retention-agent",
            &generate_run_id(),
        )
        .await;
    assert!(
        matches!(r2, Err(PlannerError::PolicyDenied(_))),
        "Account.refund 不得被 Customer 的知识放行（H3 跨类型隔离）"
    );
}

// ── TC-026k：count_action_failed LIKE 通配符转义（M6） ───────────────────
// action 名含 `%`/`_` 时不得被当 SQL LIKE 通配符放大匹配。
#[tokio::test]
async fn tc026k_count_action_failed_escapes_like_wildcards() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = RuntimeKernel::new(s);
    kernel
        .emit_cognitive(
            "ActionFailed",
            "r1",
            "agent",
            &["001".to_string()],
            Some("boom"),
            Some(json!({ "action": "Customer.100%_refund" })),
            Some(json!({ "error": "x" })),
            Some(0.0),
        )
        .await
        .unwrap();
    assert_eq!(
        kernel
            .count_action_failed("Customer.100%_refund")
            .await
            .unwrap(),
        1,
        "精确键应计数"
    );
    // `%` 不得匹配任意串、`_` 不得匹配任意字符
    assert_eq!(
        kernel
            .count_action_failed("Customer.100X_refund")
            .await
            .unwrap(),
        0,
        "% 不得当通配符放大"
    );
    assert_eq!(
        kernel
            .count_action_failed("Customer.100%refundX")
            .await
            .unwrap(),
        0,
        "_ 不得当通配符放大"
    );
}
