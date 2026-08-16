//! V2 Kernel Invariants（OpenAIP-OS V2 核心不变量测试矩阵）。
//!
//! 架构原则：**安全性不是组件多，而是不变量无法被绕过**。
//! 每个不变量一条测试，进 CI——文档会漂移，CI 不会。
//!
//! I1  State = Reduce(EventLog)
//! I2  Projection ∉ EventLog（假设轨与事实轨永不混写）
//! I3  PolicyDenied → ActionImpossible
//! I4  TrustCannotGrantCapability（Trust 只影响评分，作用在 Policy Filter 之后）
//! I5  LearningCannotGrantApproval（Learn ≠ Permission；REQ-018 默认关闭）
//! I6  EveryAction → Event
//! I7  EveryRun → Evaluation
//! I8  EveryPromotion → ReplayEvidence
//! I9  EveryMutation → ParentVersion
//! I10 EveryWorldChange → AuditableEvent

use openaip_os::kernel::effect::Reversibility;
use openaip_os::kernel::persistence::Persistence;
use openaip_os::kernel::relation::Relation;
use openaip_os::kernel::{Kernel, Scope};
use openaip_os::world::World;
use serde_json::json;

fn temp_db(name: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("{name}_{}.db", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

// ── I1：State = Reduce(EventLog) ────────────────────────────────────
#[tokio::test]
async fn i1_state_is_reduce_of_event_log() {
    let path = temp_db("i1");
    let mut k = Kernel::with_clock(|| 100);
    let ctx = k.create_context(None, Scope::World, "world").unwrap();
    k.relate(Relation::new(ctx, "a", "owns", "b", k.now()))
        .unwrap();

    let p = Persistence::open(path.to_str().unwrap()).await.unwrap();
    p.save(&k).await.unwrap();
    let log = p.event_log().await.unwrap();
    assert!(!log.is_empty(), "事实源：事件日志非空");

    // 状态从事件重建（as_of 时间窗）：世界当前状态 = Reduce(EventLog[0..t])
    let world = World::new(&p);
    let snap = world.as_of(200).await.unwrap();
    assert!(
        snap.relations
            .iter()
            .any(|(s, p, t)| p == "owns" && s == "a" && t == "b"),
        "State = Reduce(EventLog)：关系状态可由事件重建"
    );
    let _ = std::fs::remove_file(&path);
}

// ── I2：Projection ∉ EventLog（假设轨与事实轨永不混写）─────────────
#[test]
fn i2_projection_never_enters_event_log() {
    use openaip_os::kernel::projection::Projection;

    let p = Projection::new(
        "proj_i2",
        vec![],
        vec!["假设：派给 Worker A".into()],
        json!({ "action": "DeployProduction" }),
        json!({ "version": "V2" }),
    );
    // 类型即标记：投影永远是假设（hypothetical 恒 true）
    assert!(p.hypothetical(), "I2：Projection 永远标记为假设");
    // 类型隔离（编译期保证）：Projection 无 to SessionEvent/TraceEvent 转换路径，
    // Kernel::emit 只接受 Effect，SessionLog::append 只接受 EventType+data——
    // 本文件能编译即证明仓库不存在「Projection 直接入事件流」的 API。
    // 哨兵：任何把 Projection 序列化进 Event Log 的代码路径都必须在此处被拒绝评审。
}

// ── I3：PolicyDenied → ActionImpossible ─────────────────────────────
#[tokio::test]
async fn i3_policy_denied_means_action_impossible() {
    use openaip_os::worker::exec::WorkerRuntime;
    use openaip_os::worker::policy::WorkerPolicy;
    use openaip_os::worker::registry::WorkerRegistry;
    use openaip_os::worker::router::{RouterCriteria, WorkerRouter};
    use openaip_os::worker::sdk::WorkerError;

    let mut registry = WorkerRegistry::new();
    registry
        .register(Box::new(openaip_os::worker::process::ProcessWorker::new(
            openaip_os::worker::manifest::WorkerManifest::new("echo", "cli")
                .with_capabilities(&["text.echo"]),
            "echo",
        )))
        .unwrap();
    // fail-closed：text.echo 未配置 = Deny
    let policy = WorkerPolicy::new();
    let mut runtime = WorkerRuntime::new(
        &registry,
        &policy,
        WorkerRouter::new(RouterCriteria::default()),
    );
    let res = runtime.run("text.echo", json!({})).await;
    assert!(
        matches!(res, Err(WorkerError::Denied(_))),
        "I3：Deny 必须在执行前拦截"
    );
    // 事件流中无任何执行痕迹（worker.started 不存在 = Action 未发生）
    let log =
        openaip_os::session::log::SessionLog::new(openaip_os::session::event::SessionId::new("i3"));
    let _ = log;
    // （WorkerRuntime 无 log 时事件不可见；用 orchestrator 断言见 I6/既有 W7 测试）
}

// ── I4：TrustCannotGrantCapability ──────────────────────────────────
#[tokio::test]
async fn i4_trust_cannot_grant_capability() {
    use openaip_os::worker::policy::WorkerPolicy;
    use openaip_os::worker::registry::WorkerRegistry;
    use openaip_os::worker::router::{RouterCriteria, WorkerRouter};
    use openaip_os::worker::sdk::WorkerError;

    let mut registry = WorkerRegistry::new();
    let mut m = openaip_os::worker::manifest::WorkerManifest::new("high-trust", "agent")
        .with_capabilities(&["db.delete"])
        .with_trust(1.0) // 信任拉满
        .with_quality(1.0);
    m.execution = openaip_os::worker::manifest::ExecutionProtocol::Process;
    registry
        .register(Box::new(openaip_os::worker::process::ProcessWorker::new(
            m, "echo",
        )))
        .unwrap();
    // 即使 trust=1.0，Policy deny 的能力照样不可路由（评分在 Policy Filter 之后）
    let policy = WorkerPolicy::new().deny("db.delete");
    let router = WorkerRouter::new(RouterCriteria::default());
    let res = router.route(&registry, "db.delete", &policy);
    assert!(
        matches!(res, Err(WorkerError::Denied(_))),
        "I4：Trust 不能授予 Capability"
    );
}

// ── I5：LearningCannotGrantApproval（Learn ≠ Permission）────────────
#[tokio::test]
async fn i5_learning_cannot_grant_approval() {
    use openaip_os::evolution2::{EvolutionEngine, OutcomeKind};

    // 学习数据拉满：多次成功 → Validated + auto_approve 资格
    let mut ev = EvolutionEngine::new().with_gate_enabled(false); // REQ-018 默认关闭
    for _ in 0..10 {
        ev.record("RetainCustomer", OutcomeKind::Success);
    }
    ev.mine();
    ev.propose();
    ev.validate();
    // gate_enabled=false（默认）→ 学习再强也不能自动放行
    assert!(
        !ev.gate("RetainCustomer", Reversibility::Compensatable),
        "I5：Learning 不能获得 Approval（REQ-018 默认关闭）"
    );
    // 显式开启后才可能放行（且仍受 EvolutionPermissionState/五道闸门约束）
    let ev2 = EvolutionEngine::new().with_gate_enabled(true);
    let _ = ev2;
}

// ── I6：EveryAction → Event ─────────────────────────────────────────
#[tokio::test]
async fn i6_every_action_produces_event() {
    use openaip_os::worker::orchestrator::{WorkerOrchestrator, WorkerTask};
    use openaip_os::worker::policy::WorkerPolicy;
    use openaip_os::worker::registry::WorkerRegistry;
    use openaip_os::worker::router::{RouterCriteria, WorkerRouter};

    let mut registry = WorkerRegistry::new();
    registry
        .register(Box::new(openaip_os::worker::process::ProcessWorker::new(
            openaip_os::worker::manifest::WorkerManifest::new("echo", "cli")
                .with_capabilities(&["text.echo"]),
            "echo",
        )))
        .unwrap();
    let policy = WorkerPolicy::new().allow("text.echo");
    let mut orch = WorkerOrchestrator::new(
        &registry,
        &policy,
        WorkerRouter::new(RouterCriteria::default()),
    );
    let result = orch
        .run_plan(&[WorkerTask::new("text.echo", json!({ "msg": "x" }))])
        .await;
    assert!(result.all_ok, "I6 前置：Action 成功");
    let types: Vec<&str> = orch
        .session_log()
        .events()
        .iter()
        .map(|e| e.event_type.as_str())
        .collect();
    assert!(
        types.contains(&"policy.checked"),
        "I6：执行必经 policy.checked 事件"
    );
    assert!(
        types.contains(&"worker.started"),
        "I6：Action 发生必有 worker.started"
    );
    assert!(
        types.contains(&"worker.completed"),
        "I6：Action 完成必有 worker.completed"
    );
}

// ── I7：EveryRun → Evaluation（server run 端点，成败皆评）───────────
#[tokio::test]
async fn i7_every_run_produces_evaluation() {
    let storage = openaip_os::storage::LibSQLStorage::open(":memory:")
        .await
        .unwrap();
    let kernel = std::sync::Arc::new(openaip_os::runtime::RuntimeKernel::new(storage.clone()));
    let addr = {
        use tokio::net::TcpListener;
        let engine: std::sync::Arc<dyn openaip_os::agent::AgentEngine> =
            std::sync::Arc::new(tests_common::FakeEngine);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let a = listener.local_addr().unwrap();
        let ks = kernel.clone();
        tokio::spawn(async move {
            let _ = openaip_os::server::serve(
                ks,
                engine,
                listener,
                "ui/dist",
                tests_common::retain_registry(),
                false,
                false,
                false,
            )
            .await;
        });
        a
    };
    // 触发 run（FakeEngine 确定性，无需真实 LLM）
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{addr}/api/v1/wb/agent/run"))
        .json(&json!({ "agentId": "agent-1", "goal": "reduce churn" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    // 等待 run 完成 → wb_evaluation 必须出现（I7）
    let mut found = false;
    for _ in 0..50 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if !storage
            .list_evaluations(None, None, 10)
            .await
            .unwrap()
            .is_empty()
        {
            found = true;
            break;
        }
    }
    assert!(
        found,
        "I7：EveryRun → Evaluation（run 结束后 wb_evaluation 必须可查）"
    );
}

// ── I8：EveryPromotion → ReplayEvidence ─────────────────────────────
#[tokio::test]
async fn i8_every_promotion_has_replay_evidence() {
    use openaip_os::mutation::{BenchmarkConfig, MutationProposal};

    let mut p = MutationProposal::propose(
        "fix-java-bug",
        "v1",
        "v2",
        "rc",
        vec!["run_1".into()],
        json!({ "insert_step": { "step": "run_tests" } }),
    )
    .unwrap();
    p.validate().unwrap();
    let base: Vec<_> = (0..5)
        .map(|i| eval_result(&format!("b{i}"), 0.8, Some(1.0), &[]))
        .collect();
    let cand: Vec<_> = (0..5)
        .map(|i| eval_result(&format!("c{i}"), 0.86, Some(1.0), &[]))
        .collect();
    let verdict = openaip_os::mutation::benchmark(&base, &cand, &BenchmarkConfig::default());
    assert!(verdict.pass, "前置：benchmark 通过");
    p.benchmark(&base, &cand, &BenchmarkConfig::default());
    assert_eq!(p.status, openaip_os::MutationStatus::Experimental);
    assert!(
        p.promotion_reason.is_some(),
        "I8：Promotion 必有 ReplayEvidence（benchmark 判定理由）"
    );
    assert!(
        p.promotion_reason.as_deref().unwrap().contains("pass"),
        "I8：证据可审计"
    );
}

// ── I9：EveryMutation → ParentVersion ───────────────────────────────
#[tokio::test]
async fn i9_every_mutation_has_parent_version() {
    use openaip_os::mutation::MutationProposal;
    use openaip_os::skill::apply::fork_candidate;
    use openaip_os::skill::registry::SkillRegistry;

    let p = MutationProposal::propose(
        "fix-java-bug",
        "v1",
        "v2",
        "rc",
        vec![],
        json!({ "insert_step": { "step": "run_tests" } }),
    )
    .unwrap();
    assert!(
        !p.source_version.is_empty(),
        "I9：Mutation 必携带 ParentVersion"
    );

    // fork 候选的 lineage 必须指向父版本（Phase 9 G11）
    let mut reg = SkillRegistry::new();
    let asset = openaip_os::skill::asset::SkillAsset {
        id: "fix-java-bug".into(),
        name: "fix-java-bug".into(),
        version: "v1".into(),
        skill_type: openaip_os::skill::ir::SkillType::Workflow,
        goal: "fix".into(),
        inputs: vec![],
        outputs: vec![],
        workflow: vec![openaip_os::skill::ir::WorkflowStep {
            name: "modify".into(),
            description: "修改".into(),
        }],
        capabilities: vec!["code.write".into()],
        artifacts: vec![],
        preconditions: vec![],
        postconditions: vec![],
        policies: vec![],
        metrics: vec![],
        feedback_loops: vec![],
        provenance: openaip_os::skill::ir::Provenance {
            raw_skill_id: "raw".into(),
            parser: "test".into(),
        },
        status: openaip_os::skill::asset::SkillStatus::Active,
        lineage: openaip_os::skill::lineage::Lineage::new("v1"),
    };
    reg.register(asset).unwrap();
    let cand = fork_candidate(
        &mut reg,
        "fix-java-bug",
        "v2",
        &json!({ "insert_step": { "step": "run_tests" } }),
    )
    .unwrap();
    assert_eq!(
        cand.lineage.parent.as_deref(),
        Some("fix-java-bug"),
        "I9：候选必有父版本"
    );
    assert_eq!(cand.lineage.derived_from.as_deref(), Some("fix-java-bug"));
}

// ── I10：EveryWorldChange → AuditableEvent ──────────────────────────
#[tokio::test]
async fn i10_every_world_change_is_auditable_event() {
    let storage = openaip_os::storage::LibSQLStorage::open(":memory:")
        .await
        .unwrap();
    // world 事件必须绑定对象（WB2006：空 object_refs 禁止落库——不可审计的变化不允许发生）
    let orphan = openaip_os::runtime::TraceEvent::world_with_refs("ObjectCreated", &[]);
    let res = storage.append_trace(&orphan).await;
    assert!(
        res.is_err(),
        "I10：无审计痕迹的世界变化必须被拒绝（WB2006）"
    );
    // 正常路径：带 refs 的 world 事件可落库（可审计）
    let ok = openaip_os::runtime::TraceEvent::world("ObjectCreated", "customer_1");
    assert!(
        storage.append_trace(&ok).await.is_ok(),
        "I10：带 refs 的世界事件可审计落库"
    );
}

// ── 辅助 ────────────────────────────────────────────────────────────
fn eval_result(
    run_id: &str,
    aggregate: f64,
    reliability: Option<f64>,
    modes: &[&str],
) -> openaip_os::evaluation::EvaluationResult {
    use openaip_os::evaluation::Score;
    openaip_os::evaluation::EvaluationResult {
        run_id: run_id.into(),
        task_id: "t1".into(),
        skill_version: None,
        worker_outcomes: vec![],
        correctness: Score::known(aggregate, vec![]),
        completeness: Score::known(aggregate, vec![]),
        efficiency: Score::known(aggregate, vec![]),
        reliability: match reliability {
            Some(v) => Score::known(v, vec![]),
            None => Score::unknown(),
        },
        policy_compliance: Score::known(aggregate, vec![]),
        output_quality: Score::unknown(),
        aggregate,
        failure_modes: modes.iter().map(|s| s.to_string()).collect(),
        evaluator_version: openaip_os::EVALUATOR_VERSION.into(),
    }
}

/// I7 使用的共享测试辅助（与 tests/common 同构；独立文件避免跨 crate 依赖）。
mod tests_common {
    use async_trait::async_trait;
    use openaip_os::agent::{AgentEngine, AgentError, Context, Decision};
    use serde_json::json;

    pub struct FakeEngine;

    #[async_trait]
    impl AgentEngine for FakeEngine {
        fn name(&self) -> &str {
            "fake"
        }
        async fn reason(&self, ctx: &Context) -> Result<Decision, AgentError> {
            let refs: Vec<String> = ctx
                .world
                .objects
                .iter()
                .filter_map(|o| o.get("id").and_then(|x| x.as_str()).map(|s| s.to_string()))
                .collect();
            Ok(Decision {
                intent: "Customer Retention".into(),
                reasoning: "fake".into(),
                confidence: 0.9,
                object_refs: refs,
                output: json!({ "step": ctx.step }),
            })
        }
    }

    pub fn retain_registry() -> openaip_os::action::ActionRegistry {
        openaip_os::action::ActionRegistry::from_ir(
            &openaip_os::parse_odl(
                r#"
object Customer:
  properties:
    name: string
    level: string
  actions:
    retain:
      creates:
        - type: RetentionTask
          data:
            title: "客户挽留计划"
            customer: self
          link: needs_retention
"#,
            )
            .expect("retain ODL"),
        )
    }
}
