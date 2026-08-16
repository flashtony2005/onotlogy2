//! M1b · G8 契约测试：V2 Thinker（CognitiveEngine）→ WorkerTask → Orchestrator 全链。
//!
//! 契约（V1 via_workers 桥的 V2 等价）：
//! - Thinker::plan 纯规划（不执行）：goal + 可用能力 → 决策序列；
//!   重复能力 / 达到能力数上限 → 收敛（防环），None → 收敛。
//! - Decision.capability 与 Worker 能力市场同名直映（compose 命名已是点分形式），
//!   不再需要 V1 PlanStep::worker_capability 映射。
//! - to_tasks → WorkerTask{capability, input=args} → Orchestrator 执行 → 事件链完整。

use openaip_os::cognitive::engine::{AgentView, FakeEngine, NoOpEngine, PlanStep};
use openaip_os::worker::manifest::WorkerManifest;
use openaip_os::worker::orchestrator::{WorkerOrchestrator, WorkerTask};
use openaip_os::worker::policy::WorkerPolicy;
use openaip_os::worker::process::ProcessWorker;
use openaip_os::worker::registry::WorkerRegistry;
use openaip_os::worker::router::{RouterCriteria, WorkerRouter};
use openaip_os::worker::thinker::Thinker;
use serde_json::json;

fn retain_engine() -> FakeEngine {
    FakeEngine::new(vec![PlanStep {
        capability: "customer.retain".into(),
        args: json!({ "target": "u1" }),
        when: Some(Box::new(|v: &AgentView| {
            !v.has_relation("retained", "true")
        })),
    }])
}

// ── G8a：Thinker 规划 → Worker 执行 → 事件链（V1 bridge B1-B3 的 V2 等价）──
#[tokio::test]
async fn g8a_v2_thinker_plans_worker_tasks_and_executes() {
    let engine = retain_engine();
    let thinker = Thinker::new(&engine);
    let decisions = thinker.plan("reduce churn", &["customer.retain".to_string()], vec![]);
    assert_eq!(decisions.len(), 1, "防环：重复能力只出一次");
    assert_eq!(decisions[0].capability, "customer.retain");
    assert_eq!(decisions[0].args, json!({ "target": "u1" }));

    let tasks = Thinker::to_tasks(&decisions);
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].capability, "customer.retain");
    assert_eq!(
        tasks[0].input,
        json!({ "target": "u1" }),
        "args 直映 Worker input"
    );

    // Worker 执行（echo worker 注册同名能力）
    let mut registry = WorkerRegistry::new();
    registry
        .register(Box::new(ProcessWorker::new(
            WorkerManifest::new("echo-crm", "cli").with_capabilities(&["customer.retain"]),
            "echo",
        )))
        .unwrap();
    let policy = WorkerPolicy::new().allow("customer.retain");
    let mut orch = WorkerOrchestrator::new(
        &registry,
        &policy,
        WorkerRouter::new(RouterCriteria::default()),
    );
    let result = orch.run_plan(&tasks).await;
    assert!(result.all_ok, "V2 桥：Worker 执行全绿");
    let types: Vec<&str> = orch
        .session_log()
        .events()
        .iter()
        .map(|e| e.event_type.as_str())
        .collect();
    for t in ["worker.selected", "worker.started", "worker.completed"] {
        assert!(types.contains(&t), "缺事件 {t}: {types:?}");
    }
}

// ── G8b：永远有决策的引擎 → 能力数上限收敛（防环）────────────────────
#[test]
fn g8b_always_decide_engine_converges() {
    // 无 when 条件：每轮都返回同一能力
    let engine = FakeEngine::new(vec![PlanStep {
        capability: "customer.retain".into(),
        args: json!({}),
        when: None,
    }]);
    let thinker = Thinker::new(&engine);
    let decisions = thinker.plan("g", &["customer.retain".to_string()], vec![]);
    assert_eq!(decisions.len(), 1, "重复能力去重 → 收敛，不得无限循环");
}

// ── G8c：无决策引擎 → 空任务（不执行任何 Worker）─────────────────────
#[test]
fn g8c_no_decision_yields_empty_tasks() {
    let thinker = Thinker::new(&NoOpEngine);
    let decisions = thinker.plan("g", &["customer.retain".to_string()], vec![]);
    assert!(decisions.is_empty());
    assert!(Thinker::to_tasks(&decisions).is_empty());
}

// ── G8d：to_tasks 保序 + note 可读 ─────────────────────────────────────
#[test]
fn g8d_to_tasks_preserves_order() {
    let tasks: Vec<WorkerTask> = Thinker::to_tasks(&[
        openaip_os::cognitive::Decision {
            capability: "a.one".into(),
            args: json!({ "x": 1 }),
            confidence: 0.9,
            rule: None,
        },
        openaip_os::cognitive::Decision {
            capability: "b.two".into(),
            args: json!({ "y": 2 }),
            confidence: 0.8,
            rule: None,
        },
    ]);
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].capability, "a.one");
    assert_eq!(tasks[1].capability, "b.two");
    assert_eq!(tasks[1].input, json!({ "y": 2 }));
    assert!(!tasks[0].note.is_empty(), "note 应可读");
}
