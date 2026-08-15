//! V2.0 Mutation Benchmark（Phase 11 · Domain Agent Evolution）契约测试。
//!
//! MU1 结构化 Delta 白名单（自由文本改 Skill 一律拒绝）
//! MU2 配对 Benchmark 判定（效果量 / 失败率不劣 / 无新增失败模式）
//! MU3 双阶段晋升：自动只到 EXPERIMENTAL，ACTIVE 需显式激活（Human/REQ-018 在调用方）
//! MU4 失败保留：REJECTED 不删除（失败本身就是 Experience）
//! MU5 端到端：事件 → Evaluation → Benchmark → wb_mutation 持久化

use openaip_os::evaluation::{EvaluationReducer, TaskDefinition};
use openaip_os::mutation::{
    BenchmarkConfig, MutationError, MutationProposal, MutationStatus, benchmark,
};
use openaip_os::session::event::{EventType, SessionEvent, SessionId};
use openaip_os::storage::LibSQLStorage;
use serde_json::{Value, json};

fn ev(t: EventType, data: Value, seq: u64) -> SessionEvent {
    SessionEvent {
        session_id: SessionId::new("s1"),
        seq,
        event_type: t,
        timestamp: 1000 + seq as i64,
        data,
    }
}

fn task() -> TaskDefinition {
    TaskDefinition {
        task_id: "t1".into(),
        expected_relations: vec!["order.active".into()],
        expected_artifacts: vec!["report.md".into()],
        time_budget_secs: 10.0,
        skill_version: None,
    }
}

fn delta() -> Value {
    json!({
        "insert_step": { "after": "unit_test", "step": "integration_test" },
        "modify": { "step": "verify", "precondition": "regression_check" }
    })
}

/// 构造 aggregate = score 的评测结果。
fn result(
    run_id: &str,
    aggregate: f64,
    reliability: Option<f64>,
    modes: &[&str],
) -> openaip_os::evaluation::EvaluationResult {
    openaip_os::evaluation::EvaluationResult {
        run_id: run_id.into(),
        task_id: "t1".into(),
        skill_version: None,
        correctness: openaip_os::evaluation::Score::known(aggregate, vec![]),
        completeness: openaip_os::evaluation::Score::known(aggregate, vec![]),
        efficiency: openaip_os::evaluation::Score::known(aggregate, vec![]),
        reliability: match reliability {
            Some(v) => openaip_os::evaluation::Score::known(v, vec![]),
            None => openaip_os::evaluation::Score::unknown(),
        },
        policy_compliance: openaip_os::evaluation::Score::known(aggregate, vec![]),
        output_quality: openaip_os::evaluation::Score::unknown(),
        aggregate,
        failure_modes: modes.iter().map(|s| s.to_string()).collect(),
        evaluator_version: openaip_os::EVALUATOR_VERSION.into(),
    }
}

fn cfg2() -> BenchmarkConfig {
    BenchmarkConfig {
        min_runs_per_side: 2,
        ..Default::default()
    }
}

// ── MU1：结构化 Delta 白名单 ────────────────────────────────────────
#[tokio::test]
async fn tc_mu1_delta_whitelist() {
    let ok = MutationProposal::propose(
        "fix-java-bug",
        "v1",
        "v2",
        "integration test omitted",
        vec!["run_123".into(), "run_124".into()],
        delta(),
    )
    .unwrap();
    assert_eq!(ok.id, "mutation.fix-java-bug.v2");
    assert_eq!(ok.status, MutationStatus::Proposed);
    assert_eq!(ok.evidence, vec!["run_123", "run_124"]);

    // 未知操作拒绝
    let bad = MutationProposal::propose(
        "fix-java-bug",
        "v1",
        "v2",
        "x",
        vec![],
        json!({ "rewrite_step": { "step": "a" } }),
    );
    assert_eq!(bad, Err(MutationError::UnknownOp("rewrite_step".into())));

    // 自由文本拒绝（AI 直接改 Skill 的路径被结构挡死）
    let free_text =
        MutationProposal::propose("fix-java-bug", "v1", "v2", "x", vec![], json!("free text"));
    assert_eq!(free_text, Err(MutationError::DeltaNotObject));
}

// ── MU2：配对 Benchmark 判定 ────────────────────────────────────────
#[tokio::test]
async fn tc_mu2_benchmark_rules() {
    let base: Vec<_> = (0..3)
        .map(|i| result(&format!("b{i}"), 0.70, Some(1.0), &[]))
        .collect();
    let cand: Vec<_> = (0..3)
        .map(|i| result(&format!("c{i}"), 0.85, Some(1.0), &[]))
        .collect();
    let v = benchmark(&base, &cand, &cfg2());
    assert!(v.pass, "{}", v.reason);
    assert!((v.effect_size - 0.15).abs() < 1e-9);
    assert_eq!(v.baseline_runs, 3);
    assert_eq!(v.candidate_runs, 3);
    assert!(v.new_failure_modes.is_empty());

    // 效果量不足
    let flat: Vec<_> = (0..3)
        .map(|i| result(&format!("c{i}"), 0.72, Some(1.0), &[]))
        .collect();
    assert!(!benchmark(&base, &flat, &cfg2()).pass);

    // 失败率劣化
    let shaky: Vec<_> = (0..3)
        .map(|i| result(&format!("c{i}"), 0.90, Some(0.4), &[]))
        .collect();
    let v2 = benchmark(&base, &shaky, &cfg2());
    assert!(!v2.pass);
    assert!(v2.reason.contains("失败率"));

    // 新增失败模式
    let newmode: Vec<_> = (0..3)
        .map(|i| result(&format!("c{i}"), 0.90, Some(1.0), &["integration timeout"]))
        .collect();
    let v3 = benchmark(&base, &newmode, &cfg2());
    assert!(!v3.pass);
    assert_eq!(
        v3.new_failure_modes,
        vec!["integration timeout".to_string()]
    );

    // 样本不足（生产默认 ≥5）
    let tiny = vec![result("c0", 0.99, Some(1.0), &[])];
    let v4 = benchmark(&base, &tiny, &BenchmarkConfig::default());
    assert!(!v4.pass);
    assert!(v4.reason.contains("runs"));
}

// ── MU3：双阶段晋升（EXPERIMENTAL 自动；ACTIVE 需显式）─────────────
#[tokio::test]
async fn tc_mu3_two_stage_promotion() {
    let mut p =
        MutationProposal::propose("fix-java-bug", "v1", "v2", "rc", vec![], delta()).unwrap();
    p.validate().unwrap();
    assert_eq!(p.status, MutationStatus::Validated);

    let base: Vec<_> = (0..3)
        .map(|i| result(&format!("b{i}"), 0.70, Some(1.0), &[]))
        .collect();
    let cand: Vec<_> = (0..3)
        .map(|i| result(&format!("c{i}"), 0.85, Some(1.0), &[]))
        .collect();
    let v = p.benchmark(&base, &cand, &cfg2());
    assert!(v.pass);
    assert_eq!(
        p.status,
        MutationStatus::Experimental,
        "自动晋升只到 EXPERIMENTAL"
    );

    // 未经过 Human Approval / REQ-018 时不能 ACTIVE——本层不持有权限（Learn ≠ Permission）
    // 模拟调用方已通过审批后显式激活
    p.activate().unwrap();
    assert_eq!(p.status, MutationStatus::Active);

    // 重复激活非法
    assert!(p.activate().is_err());
}

// ── MU4：失败保留（REJECTED 不删除）────────────────────────────────
#[tokio::test]
async fn tc_mu4_failure_kept() {
    let mut p =
        MutationProposal::propose("fix-java-bug", "v1", "v2", "rc", vec![], delta()).unwrap();
    p.validate().unwrap();
    let base: Vec<_> = (0..3)
        .map(|i| result(&format!("b{i}"), 0.80, Some(1.0), &[]))
        .collect();
    let cand: Vec<_> = (0..3)
        .map(|i| result(&format!("c{i}"), 0.81, Some(1.0), &[]))
        .collect();
    let v = p.benchmark(&base, &cand, &cfg2());
    assert!(!v.pass);
    assert_eq!(p.status, MutationStatus::Rejected);
    assert!(p.promotion_reason.is_some(), "判定理由保留（可追溯）");
    // REJECTED 是保留态：不能直接 ACTIVE，但可归档（失败也是 Experience）
    assert!(p.activate().is_err());
    p.archive();
    assert_eq!(p.status, MutationStatus::Archived);
}

// ── MU5：端到端（事件 → Evaluation → Benchmark → wb_mutation）──────
#[tokio::test]
async fn tc_mu5_end_to_end_persisted() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    let reducer = EvaluationReducer::default();
    let t = task();

    // 同一任务集：v1 慢（20s 超预算），v2 快（2s）
    let base_events = vec![
        ev(
            EventType::WorkerStarted,
            json!({ "worker": "w1", "attempt": 1 }),
            0,
        ),
        ev(
            EventType::WorkerCompleted,
            json!({ "worker": "w1", "ok": true, "duration_ms": 20000.0 }),
            1,
        ),
        ev(
            EventType::SessionCompleted,
            json!({ "relation": "order.active" }),
            2,
        ),
    ];
    let cand_events = vec![
        ev(
            EventType::WorkerStarted,
            json!({ "worker": "w1", "attempt": 1 }),
            0,
        ),
        ev(
            EventType::WorkerCompleted,
            json!({ "worker": "w1", "ok": true, "duration_ms": 2000.0 }),
            1,
        ),
        ev(
            EventType::SessionCompleted,
            json!({ "relation": "order.active" }),
            2,
        ),
    ];
    let base: Vec<_> = (0..3)
        .map(|i| reducer.evaluate(&base_events, &format!("b{i}"), &t))
        .collect();
    let cand: Vec<_> = (0..3)
        .map(|i| reducer.evaluate(&cand_events, &format!("c{i}"), &t))
        .collect();

    // 评测结果落 wb_evaluation（派生表）
    for r in base.iter().chain(cand.iter()) {
        storage.save_evaluation(r).await.unwrap();
    }
    assert_eq!(
        storage
            .list_evaluations(None, None, 100)
            .await
            .unwrap()
            .len(),
        6
    );

    // 提案 → 校验 → Benchmark → EXPERIMENTAL → 落 wb_mutation
    let mut p = MutationProposal::propose(
        "fix-java-bug",
        "v1",
        "v2",
        "integration test omitted",
        vec!["b0".into(), "b1".into()],
        delta(),
    )
    .unwrap();
    p.validate().unwrap();
    let v = p.benchmark(&base, &cand, &cfg2());
    assert!(v.pass, "{}", v.reason);
    assert_eq!(p.status, MutationStatus::Experimental);

    storage.save_mutation(&p).await.unwrap();
    let persisted = storage.list_mutations(None).await.unwrap();
    assert_eq!(persisted.len(), 1);
    assert_eq!(persisted[0].id, "mutation.fix-java-bug.v2");
    assert_eq!(persisted[0].status, MutationStatus::Experimental);
    assert_eq!(
        persisted[0].delta["insert_step"]["step"],
        json!("integration_test")
    );

    // 状态过滤查询
    let exps = storage
        .list_mutations(Some(MutationStatus::Experimental))
        .await
        .unwrap();
    assert_eq!(exps.len(), 1);
    let actives = storage
        .list_mutations(Some(MutationStatus::Active))
        .await
        .unwrap();
    assert!(actives.is_empty());

    // 激活后幂等 upsert 覆盖
    p.activate().unwrap();
    storage.save_mutation(&p).await.unwrap();
    let after = storage.list_mutations(None).await.unwrap();
    assert_eq!(after.len(), 1, "同 id 覆盖不重复");
    assert_eq!(after[0].status, MutationStatus::Active);
}
