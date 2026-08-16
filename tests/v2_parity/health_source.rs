//! M1b · G7 契约测试：Loop Health 数据源 = V2 路径写入的 run 元数据。
//!
//! 契约：wb_run 重定位为 V2 run store（storage/runs.rs 已与 planner 解耦）；
//! V1 路径删除后，health::collect 的六指标仍从 V2 入口写入的数据派生
//! （I1 State = Reduce(EventLog) 原则下的诚实边界）。

use openaip_os::evaluation::{EvaluationReducer, TaskDefinition};
use openaip_os::health::collect;
use openaip_os::runtime::{TraceEvent, TraceKind};
use openaip_os::session::event::{EventType, SessionId};
use openaip_os::session::log::SessionLog;
use openaip_os::session::run::RunStatus;
use openaip_os::storage::LibSQLStorage;
use serde_json::json;

#[tokio::test]
async fn g7a_health_derives_from_v2_written_run_rows() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();

    // V2 入口写入 run 元数据（状态字符串来自 session::run::RunStatus）
    for (i, status) in [
        RunStatus::Completed,
        RunStatus::Completed,
        RunStatus::Failed,
    ]
    .iter()
    .enumerate()
    {
        let rid = format!("run_g7_{i}");
        storage.insert_run(&rid, "agent", "goal").await.unwrap();
        storage
            .update_run_status(&rid, status.as_str(), None)
            .await
            .unwrap();
    }

    // 认知追踪事件：run_g7_0 一条 Action 成功 + 一条 PolicyCheck Allow
    for (i, (ty, out)) in [
        ("Action", json!("Success")),
        ("PolicyCheck", json!("Allow")),
    ]
    .iter()
    .enumerate()
    {
        storage
            .append_trace(&TraceEvent {
                id: format!("tr_cog_{i}"),
                kind: TraceKind::Cognitive,
                event_type: (*ty).into(),
                object_refs: vec!["u1".into()],
                run_id: Some("run_g7_0".into()),
                agent_id: Some("agent".into()),
                reasoning: None,
                input: None,
                output: Some(out.clone()),
                confidence: None,
                timestamp: "0".into(),
            })
            .await
            .unwrap();
    }
    // 世界事件：run_g7_1
    storage
        .append_trace(&TraceEvent {
            id: "tr_world_0".into(),
            kind: TraceKind::World,
            event_type: "ObjectCreated".into(),
            object_refs: vec!["u2".into()],
            run_id: Some("run_g7_1".into()),
            agent_id: None,
            reasoning: None,
            input: None,
            output: None,
            confidence: None,
            timestamp: "0".into(),
        })
        .await
        .unwrap();

    // I7 闭环：run_g7_0 有评估
    let reducer = EvaluationReducer::default();
    let task = TaskDefinition {
        task_id: "t".into(),
        expected_relations: vec![],
        expected_artifacts: vec![],
        time_budget_secs: 0.0,
        skill_version: None,
    };
    let mut log = SessionLog::new(SessionId::new("run_g7_0"));
    log.append(EventType::WorkerCompleted, json!({ "ok": true }));
    let r = reducer.evaluate(log.events(), "run_g7_0", &task);
    storage.save_evaluation(&r).await.unwrap();

    let health = collect(&storage).await.unwrap();
    assert_eq!(
        health.execution.total_runs, 3,
        "total_runs 来自 V2 写入的 run 行"
    );
    assert_eq!(health.execution.closure_rate, 1.0 / 3.0, "1/3 有评估");
    assert_eq!(health.execution.action_success_rate, 1.0, "Action 全成功");
    assert_eq!(
        health.execution.world_change_rate,
        1.0 / 3.0,
        "1/3 有世界变化"
    );
}

#[tokio::test]
async fn g7b_health_survives_empty_world() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    // 无 run、无事件、无评估 → 六指标全部可计算（分母零保护，不 panic）
    let health = collect(&storage).await.unwrap();
    assert_eq!(health.execution.total_runs, 0);
    assert_eq!(health.execution.closure_rate, 0.0);
    assert_eq!(health.execution.action_success_rate, 0.0);
    assert_eq!(health.execution.world_change_rate, 0.0);
}
