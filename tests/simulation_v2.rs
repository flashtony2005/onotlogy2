//! S1 Decision Replay 集成契约：当前 Trust 重跑历史路由决策（Learning Counterfactual）。
//!
//! R1 历史 actual 选择 vs 当前经验重放 → changed 报告（可审计评分）
//! R2 Replay 永不改变历史 EventLog（Projection 假设轨）
//! R3 数据全持久化：wb_worker（declared）+ wb_worker_stats（learned）+ wb_evaluation（actual）

use openaip_os::evaluation::{EvaluationReducer, TaskDefinition};
use openaip_os::session::event::{EventType, SessionId};
use openaip_os::session::log::SessionLog;
use openaip_os::storage::LibSQLStorage;
use serde_json::json;

fn worker(id: &str, quality: f64, trust: f64) -> openaip_os::worker::manifest::WorkerManifest {
    openaip_os::worker::manifest::WorkerManifest::new(id, "agent")
        .with_capabilities(&["code.read"])
        .with_quality(quality)
        .with_trust(trust)
}

#[tokio::test]
async fn decision_replay_flips_choice_with_learned_trust() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();

    // wb_worker：两个同质候选（declared 相同）
    storage.save_worker(&worker("w-a", 0.9, 0.8)).await.unwrap();
    storage.save_worker(&worker("w-b", 0.9, 0.8)).await.unwrap();

    // wb_worker_stats：w-a 差（2/10）、w-b 好（10/10）→ learned ±0.1
    for i in 0..10 {
        storage
            .record_worker_outcome("code.read", "w-a", i < 2, 100)
            .await
            .unwrap();
        storage
            .record_worker_outcome("code.read", "w-b", true, 100)
            .await
            .unwrap();
    }

    // wb_evaluation.worker_outcomes：历史实际选择 = w-a（当时的决定）
    let reducer = EvaluationReducer::default();
    let task = TaskDefinition {
        task_id: "t1".into(),
        expected_relations: vec![],
        expected_artifacts: vec![],
        time_budget_secs: 10.0,
        skill_version: None,
    };
    let mut log = SessionLog::new(SessionId::new("run_182"));
    log.append(
        EventType::WorkerStarted,
        json!({ "worker": "w-a", "attempt": 1 }),
    );
    log.append(
        EventType::WorkerCompleted,
        json!({ "worker": "w-a", "capability": "code.read", "ok": true, "duration_ms": 100.0 }),
    );
    let eval = reducer.evaluate(log.events(), "run_182", &task);
    assert_eq!(eval.worker_outcomes.len(), 1);
    assert_eq!(eval.worker_outcomes[0].worker_id, "w-a", "历史实际选择");
    storage.save_evaluation(&eval).await.unwrap();

    // R1：重放 → 当前经验下应选 w-b
    let report = openaip_os::simulation::replay::run_decision_replay(&storage)
        .await
        .unwrap();
    assert_eq!(report.total_decisions, 1);
    assert_eq!(report.changed, 1);
    assert_eq!(report.change_rate, 1.0);
    let d = &report.decisions[0];
    assert_eq!(d.actual_worker, "w-a");
    assert_eq!(d.replay_worker, "w-b");
    assert!(
        d.scores
            .iter()
            .any(|(id, s)| id == "w-b" && (*s - 0.81).abs() < 1e-9),
        "w-b 评分 = 0.9 × (0.8+0.1) = 0.81: {:?}",
        d.scores
    );
    assert!(
        d.scores
            .iter()
            .any(|(id, s)| id == "w-a" && (*s - 0.63).abs() < 1e-9),
        "w-a 评分 = 0.9 × (0.8−0.1) = 0.63: {:?}",
        d.scores
    );

    // R2：Replay 永不改变历史（Projection 假设轨；历史事件流原封不动）
    let p = openaip_os::simulation::replay::report_projection(&report);
    assert!(p.hypothetical(), "I2：重放产物是 Projection");
    let loaded = storage
        .list_evaluations(Some("run_182"), None, 10)
        .await
        .unwrap();
    assert_eq!(
        loaded[0].worker_outcomes[0].worker_id, "w-a",
        "历史未被重放改写"
    );
}

#[tokio::test]
async fn replay_without_experience_is_unchanged() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    storage.save_worker(&worker("w-a", 0.9, 0.8)).await.unwrap();
    // 无 stats（无经验）→ learned 无修正 → 重放不变
    let reducer = EvaluationReducer::default();
    let task = TaskDefinition {
        task_id: "t1".into(),
        expected_relations: vec![],
        expected_artifacts: vec![],
        time_budget_secs: 10.0,
        skill_version: None,
    };
    let mut log = SessionLog::new(SessionId::new("run_1"));
    log.append(
        EventType::WorkerStarted,
        json!({ "worker": "w-a", "attempt": 1 }),
    );
    log.append(
        EventType::WorkerCompleted,
        json!({ "worker": "w-a", "capability": "code.read", "ok": true, "duration_ms": 100.0 }),
    );
    storage
        .save_evaluation(&reducer.evaluate(log.events(), "run_1", &task))
        .await
        .unwrap();
    let report = openaip_os::simulation::replay::run_decision_replay(&storage)
        .await
        .unwrap();
    assert_eq!(report.changed, 0, "无经验不改变选择");
}

// ── S2/S3：Projection Simulation 端到端（真实事件源状态 as_of → 假设投影）──
#[tokio::test]
async fn projection_uses_event_sourced_state() {
    use openaip_os::kernel::effect::Reversibility;
    use openaip_os::kernel::persistence::Persistence;
    use openaip_os::kernel::relation::Relation;
    use openaip_os::kernel::{Kernel, Scope};
    use openaip_os::simulation::projection::{HypotheticalAction, counterfactual, project};
    use openaip_os::world::World;

    // 事件源世界：Kernel 写关系 → Persistence 落盘 → as_of(t) 重建 State
    let path = std::env::temp_dir().join(format!("s2_{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let mut k = Kernel::with_clock(|| 100);
    let ctx = k.create_context(None, Scope::World, "world").unwrap();
    k.relate(Relation::new(ctx, "order_1", "owns", "customer_1", k.now()))
        .unwrap();
    k.relate(Relation::new(
        ctx,
        "customer_1",
        "status",
        "active",
        k.now(),
    ))
    .unwrap();
    let p = Persistence::open(path.to_str().unwrap()).await.unwrap();
    p.save(&k).await.unwrap();
    let snap = World::new(&p).as_of(1000).await.unwrap();
    let state: Vec<(String, String, String)> = snap.relations;

    // S2：假设动作投影到真实事件源状态
    let mut deploy = HypotheticalAction::new(
        "DeployProduction",
        Some("version"),
        Some("V2"),
        Reversibility::Irreversible,
    );
    deploy.capability = Some("deploy.production".into());
    let proj = project(&state, &deploy);
    assert!(proj.hypothetical(), "I2：投影是假设轨");
    assert_eq!(
        proj.predicted_state["relations"].as_array().unwrap().len(),
        3,
        "2 真实 + 1 假设"
    );
    assert_eq!(
        proj.predicted_state["added"][0],
        json!(["DeployProduction", "version", "V2"])
    );

    // S3：双动作对比（deny 侧出局 → 推荐 canary）
    let policy = openaip_os::worker::policy::WorkerPolicy::new().allow("deploy.production");
    let mut canary = HypotheticalAction::new(
        "CanaryDeploy",
        Some("version"),
        Some("V2-canary"),
        Reversibility::Compensatable,
    );
    canary.capability = Some("deploy.production".into());
    let report = counterfactual(&state, &deploy, &canary, &policy, &[]);
    assert_eq!(
        report.recommendation.as_deref(),
        Some("CanaryDeploy"),
        "低风险优先"
    );

    // 事件源不被假设污染：重放状态仍是 2 条关系
    let snap2 = World::new(&p).as_of(1000).await.unwrap();
    assert_eq!(snap2.relations.len(), 2, "I2：假设投影永不写入事件源");
    let _ = std::fs::remove_file(&path);
}

// ── S4：Simulation 状态来源（真实世界关系集）────────────────────────
#[tokio::test]
async fn simulate_state_comes_from_real_relations() {
    use openaip_os::storage::RelationEngine;

    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    storage
        .link("order_1", "owns", "customer_1", None)
        .await
        .unwrap();
    let rels = storage.list_relations().await.unwrap();
    assert_eq!(rels.len(), 1);
    assert_eq!(
        (
            rels[0].source.as_str(),
            rels[0].relation.as_str(),
            rels[0].target.as_str()
        ),
        ("order_1", "owns", "customer_1")
    );

    // 状态 → S2 投影（Decision Support 基于当前世界）
    let state: Vec<(String, String, String)> = rels
        .iter()
        .map(|r| (r.source.clone(), r.relation.clone(), r.target.clone()))
        .collect();
    let mut deploy = openaip_os::simulation::projection::HypotheticalAction::new(
        "DeployProduction",
        Some("version"),
        Some("V2"),
        openaip_os::kernel::effect::Reversibility::Irreversible,
    );
    deploy.capability = Some("deploy.production".into());
    let proj = openaip_os::simulation::projection::project(&state, &deploy);
    assert_eq!(
        proj.predicted_state["relations"].as_array().unwrap().len(),
        2,
        "1 真实 + 1 假设"
    );
}
