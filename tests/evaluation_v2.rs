//! V2.0 Evaluation（Phase 11 · Domain Agent Evolution）契约测试。
//!
//! 第一原则：**Evaluation = Pure Function(Event Log)**——Deterministic / Idempotent / Replayable。
//!
//! EV1 六维评测：客观维度从事件确定性计算（efficiency / reliability / policy_compliance）
//! EV2 同一事件集永远同一分数（Deterministic / Idempotent / Replayable）
//! EV3 judge.submitted 进入事实层（主观维度 + 半客观修正；Judge 本身可审计）
//! EV4 context.assembled 进入事实层（Context 归因前提）
//! EV5 Fact/Judgment 分离：客观维度绝不依赖 judge（无 judge 照常计算）
//! EV6 wb_evaluation 持久化：派生表语义（save → list → 重算覆盖一致）

use openaip_os::evaluation::{EvaluationPolicy, EvaluationReducer, TaskDefinition};
use openaip_os::session::event::{EventType, SessionEvent, SessionId};
use openaip_os::session::log::SessionLog;
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
    }
}

// ── EV1：六维评测（客观维度确定性计算）──────────────────────────────
#[tokio::test]
async fn tc_ev1_six_dimensions() {
    let events = vec![
        ev(
            EventType::PolicyChecked,
            json!({ "capability": "code.test", "verdict": "allow" }),
            0,
        ),
        ev(
            EventType::WorkerSelected,
            json!({ "worker": "w1", "capability": "code.test" }),
            1,
        ),
        ev(
            EventType::WorkerStarted,
            json!({ "worker": "w1", "capability": "code.test", "attempt": 1 }),
            2,
        ),
        ev(
            EventType::WorkerCompleted,
            json!({ "worker": "w1", "capability": "code.test", "ok": true, "duration_ms": 20000.0 }),
            3,
        ),
        ev(
            EventType::ArtifactCreated,
            json!({ "artifact": "report.md" }),
            4,
        ),
        ev(
            EventType::SessionCompleted,
            json!({ "relation": "order.active" }),
            5,
        ),
    ];
    let r = EvaluationReducer::default().evaluate(&events, "run_1", &task());
    assert_eq!(r.correctness, Some(1.0), "expected_relations 命中");
    assert_eq!(r.completeness, Some(1.0), "artifact.created 命中");
    assert_eq!(r.efficiency, Some(0.5), "20000ms / 10s 预算");
    assert_eq!(r.reliability, Some(1.0));
    assert_eq!(r.policy_compliance, Some(1.0));
    assert_eq!(r.output_quality, None, "无 judge 不猜测");
    assert_eq!(r.aggregate, (1.0 + 1.0 + 0.5 + 1.0 + 1.0) / 5.0);
    assert!(r.failure_modes.is_empty());
}

// ── EV2：Deterministic / Idempotent / Replayable ─────────────────────
#[tokio::test]
async fn tc_ev2_deterministic_idempotent_replayable() {
    let events = vec![
        ev(
            EventType::WorkerStarted,
            json!({ "worker": "w1", "attempt": 1 }),
            0,
        ),
        ev(
            EventType::WorkerFailed,
            json!({ "worker": "w1", "error": "timeout" }),
            1,
        ),
        ev(
            EventType::WorkerStarted,
            json!({ "worker": "w1", "attempt": 2 }),
            2,
        ),
        ev(
            EventType::WorkerCompleted,
            json!({ "worker": "w1", "ok": true, "duration_ms": 8000.0 }),
            3,
        ),
        ev(
            EventType::PolicyChecked,
            json!({ "capability": "db.delete", "verdict": "deny" }),
            4,
        ),
    ];
    let r1 = EvaluationReducer::default().evaluate(&events, "run_1", &task());
    let r2 = EvaluationReducer::default().evaluate(&events, "run_1", &task());
    assert_eq!(r1, r2, "同一事件集永远同一分数");
    // 可回放：删除评测结果后可由事件重建（此处直接重算）
    assert_eq!(r2.reliability, Some(0.5));
    assert_eq!(r2.policy_compliance, Some(0.0));
    assert_eq!(r2.failure_modes, vec!["timeout".to_string()]);
}

// ── EV3：judge.submitted 是事实（主观维度 + 半客观修正）─────────────
#[tokio::test]
async fn tc_ev3_judge_is_fact() {
    let mut log = SessionLog::new(SessionId::new("s1"));
    log.append(
        EventType::JudgeSubmitted,
        json!({
            "judge": "human",
            "dimension": "output_quality",
            "score": 0.86,
            "rubric_version": "rubric.v1",
            "confidence": 0.9
        }),
    );
    log.append(
        EventType::JudgeSubmitted,
        json!({
            "judge": "llm",
            "dimension": "correctness",
            "score": 0.91,
            "confidence": 0.8
        }),
    );
    let events: Vec<SessionEvent> = log.events().to_vec();
    let r = EvaluationReducer::default().evaluate(&events, "run_1", &task());
    assert_eq!(r.output_quality, Some(0.86));
    assert_eq!(r.correctness, Some(0.91), "judge 修正半客观维度");
    // judge 事件本身 append-only 可回放（judge 成为可审计事实）
    assert_eq!(
        events.last().unwrap().event_type.as_str(),
        "judge.submitted"
    );
}

// ── EV4：context.assembled 是事实（Context 归因前提）────────────────
#[tokio::test]
async fn tc_ev4_context_assembled_is_fact() {
    let mut log = SessionLog::new(SessionId::new("s1"));
    log.append(
        EventType::ContextAssembled,
        json!({
            "run_id": "run_123",
            "assets": [
                { "asset_id": "java.architecture", "selection_reason": "project_type=java", "relevance": 0.91 },
                { "asset_id": "integration.testing", "selection_reason": "task.requires_testing", "relevance": 0.87 }
            ]
        }),
    );
    let events = log.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type.as_str(), "context.assembled");
    let assets = events[0].data["assets"].as_array().unwrap();
    assert_eq!(assets.len(), 2);
    assert_eq!(assets[0]["asset_id"], json!("java.architecture"));
    // 事件进入评测（不影响客观维度；为 Context 相关性分析提供归因事实）
    let r = EvaluationReducer::default().evaluate(events, "run_123", &task());
    assert_eq!(r.run_id, "run_123");
}

// ── EV5：客观维度绝不依赖 judge（无 judge 照常计算）────────────────
#[tokio::test]
async fn tc_ev5_objective_without_judge() {
    let events = vec![
        ev(
            EventType::WorkerStarted,
            json!({ "worker": "w1", "attempt": 1 }),
            0,
        ),
        ev(
            EventType::WorkerFailed,
            json!({ "worker": "w1", "error": "boom" }),
            1,
        ),
        ev(
            EventType::WorkerStarted,
            json!({ "worker": "w1", "attempt": 2 }),
            2,
        ),
        ev(
            EventType::WorkerCompleted,
            json!({ "worker": "w1", "ok": true, "duration_ms": 3000.0 }),
            3,
        ),
    ];
    let r = EvaluationReducer::default().evaluate(&events, "run_1", &task());
    assert_eq!(r.reliability, Some(0.5), "客观维度从事件直接计算");
    assert_eq!(r.efficiency, Some(1.0));
    assert_eq!(r.policy_compliance, Some(1.0), "无违规事件 → 合规");
    assert_eq!(r.output_quality, None);
}

// ── EV6：wb_evaluation 持久化（派生表语义）──────────────────────────
#[tokio::test]
async fn tc_ev6_persistence_derived_table() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    let reducer = EvaluationReducer::new(EvaluationPolicy::default());
    let events = vec![
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
    let r = reducer.evaluate(&events, "run_9", &task());
    storage.save_evaluation(&r).await.unwrap();

    let loaded = storage.list_evaluations(Some("run_9"), 100).await.unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].aggregate, r.aggregate);
    assert_eq!(loaded[0].failure_modes, r.failure_modes);

    // 幂等 upsert：同 run 重算覆盖（派生表语义）
    let r2 = reducer.evaluate(&events, "run_9", &task());
    storage.save_evaluation(&r2).await.unwrap();
    let loaded2 = storage.list_evaluations(None, 100).await.unwrap();
    assert_eq!(loaded2.len(), 1, "同 run 覆盖，不重复");
}
