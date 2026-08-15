//! V2 桥（三环接线的分水岭）：Thinker 规划 → Worker ABI 执行 → 评测。
//!
//! B1 plan() 只规划不执行（与 ActionEngine 路径解耦）
//! B2 PlanStep::worker_capability：Action 变体 → `{object_type}.{action}`（kernel 内部操作不过桥）
//! B3 全链：FakeEngine 规划 → WorkerTask → Orchestrator（echo worker）→ 事件 → on_run_end
//! B4 skill_version 进评测（learning_effectiveness 的观测前提）

mod common;

use openaip_os::evaluation::{EvaluationPolicy, TaskDefinition};
use openaip_os::planner::Planner;
use openaip_os::runtime::RuntimeKernel;
use openaip_os::session::event::EventType;
use openaip_os::storage::LibSQLStorage;
use openaip_os::worker::orchestrator::{WorkerOrchestrator, WorkerTask};
use openaip_os::worker::policy::WorkerPolicy;
use openaip_os::worker::process::ProcessWorker;
use openaip_os::worker::registry::WorkerRegistry;
use openaip_os::worker::router::{RouterCriteria, WorkerRouter};
use serde_json::json;

#[tokio::test]
async fn bridge_thinker_plan_to_worker_execution() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = RuntimeKernel::new(storage.clone());
    kernel
        .create_object("Customer", "001", json!({"name": "Apple", "level": "VIP"}))
        .await
        .unwrap();

    // B1：Thinker 规划（只规划不执行——不产生任何 Action/world 副作用事件之外的执行）
    let engine = common::FakeEngine;
    let planner = Planner::with_actions(&kernel, common::retain_registry());
    let run_id = openaip_os::planner::generate_run_id();
    let planned = planner
        .plan(&engine, "reduce churn", "agent-1", &run_id)
        .await
        .expect("Thinker 规划");
    assert!(!planned.steps.is_empty(), "B1：计划非空");

    // B2：桥映射——Action 变体过桥，kernel 内部操作不过桥
    let caps: Vec<String> = planned
        .steps
        .iter()
        .filter_map(|s| s.worker_capability())
        .collect();
    assert!(
        caps.iter().any(|c| c == "customer.retain"),
        "B2：Action → {{object_type}}.{{action}}，实际 {caps:?}"
    );

    // B3：Worker 执行（能力市场注册支持计划能力的 echo worker）
    let mut registry = WorkerRegistry::new();
    registry
        .register(Box::new(ProcessWorker::new(
            openaip_os::worker::manifest::WorkerManifest::new("echo-crm", "cli")
                .with_capabilities(&caps.iter().map(|c| c.as_str()).collect::<Vec<&str>>()),
            "echo",
        )))
        .unwrap();
    let mut policy = WorkerPolicy::new();
    for c in &caps {
        policy = policy.allow(c);
    }
    let mut orch = WorkerOrchestrator::new(
        &registry,
        &policy,
        WorkerRouter::new(RouterCriteria::default()),
    );
    let tasks: Vec<WorkerTask> = planned
        .steps
        .iter()
        .filter_map(|s| {
            s.worker_capability().map(|cap| WorkerTask {
                capability: cap,
                input: json!({ "object_refs": s.object_refs }),
                note: s.description.clone(),
            })
        })
        .collect();
    let result = orch.run_plan(&tasks).await;
    assert!(result.all_ok, "B3：Worker 执行全绿");
    assert!(
        result.outcomes.iter().all(|o| o.worker_id.is_some()),
        "B3：每一步有 Worker 归属"
    );
    // 事件链完整（Thinker 计划被 Worker 执行的事实可回放）
    let types: Vec<&str> = orch
        .session_log()
        .events()
        .iter()
        .map(|e| e.event_type.as_str())
        .collect();
    for t in [
        "capability.selected",
        "policy.checked",
        "worker.started",
        "worker.completed",
    ] {
        assert!(types.contains(&t), "B3：缺事件 {t}");
    }

    // B4：skill_version 进评测（I7 观测：EveryRun→Evaluation 携带 skill 身份）
    orch.session_log()
        .append(EventType::SessionCompleted, json!({ "run_id": run_id }));
    let task = TaskDefinition {
        task_id: "reduce churn".into(),
        expected_relations: vec![],
        expected_artifacts: vec![],
        time_budget_secs: 10.0,
        skill_version: Some("fix-java-bug@v1".into()),
    };
    let eval = openaip_os::on_run_end(
        orch.session_log(),
        &run_id,
        &task,
        &EvaluationPolicy::default(),
        &storage,
    )
    .await
    .expect("评测");
    assert_eq!(eval.skill_version.as_deref(), Some("fix-java-bug@v1"));
    assert_eq!(eval.reliability.value, Some(1.0));
    assert_eq!(eval.policy_compliance.value, Some(1.0));

    // skill_version 持久化（learning_effectiveness 的数据基础）
    let loaded = storage
        .list_evaluations(Some(&run_id), None, 10)
        .await
        .unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(
        loaded[0].skill_version.as_deref(),
        Some("fix-java-bug@v1"),
        "B4：wb_evaluation.skill_version 携带 Skill 身份"
    );
}
