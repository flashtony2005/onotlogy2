//! worker run（M5 拆分）：能力市场 + 路由 + 策略 + 执行 + 评测。

pub fn default_worker_env() -> (
    openaip_os::worker::registry::WorkerRegistry,
    openaip_os::worker::policy::WorkerPolicy,
) {
    use openaip_os::worker::adapters;
    use openaip_os::worker::manifest::WorkerManifest;
    use openaip_os::worker::process::ProcessWorker;

    let mut registry = openaip_os::worker::registry::WorkerRegistry::new();
    let _ = registry.register(Box::new(ProcessWorker::new(
        WorkerManifest::new("echo", "cli").with_capabilities(&["text.echo"]),
        "echo",
    )));
    let _ = registry.register(Box::new(ProcessWorker::new(
        WorkerManifest::new("git", "cli")
            .with_capabilities(&["git.version", "git.status", "git.diff"])
            .with_quality(0.98),
        "git",
    )));
    let _ = registry.register(Box::new(adapters::codex_worker(None)));
    let _ = registry.register(Box::new(adapters::claude_worker(None)));
    let dsh_cmd = std::env::var("OPENAI_OS_DSH_CMD").unwrap_or_else(|_| "dsh".into());
    let _ = registry.register(Box::new(adapters::dsh_worker(Some(&dsh_cmd))));
    let pi_cmd = std::env::var("OPENAI_OS_PI_CMD").unwrap_or_else(|_| "pi".into());
    let _ = registry.register(Box::new(adapters::pi_worker(Some(&pi_cmd))));
    (registry, openaip_os::worker::policy::WorkerPolicy::new())
}

/// 按计划能力构造 fail-closed policy（shell.execute/code.write 需审批）。
pub fn policy_for_capabilities(caps: &[String]) -> openaip_os::worker::policy::WorkerPolicy {
    use openaip_os::worker::policy::WorkerPolicy;
    let mut policy = WorkerPolicy::new();
    for c in caps {
        match c.as_str() {
            "shell.execute" | "code.write" => policy = policy.require_approval(c),
            _ => policy = policy.allow(c),
        }
    }
    policy
}

/// Worker ABI 执行（Phase 11D）：capability 计划 → Router → Policy → Worker → Event Log → Evaluation。
/// OpenAIP-OS 是 Thinker：只选择谁来做、怎么安全地做，不亲自执行。
pub async fn cmd_worker_run(
    db: &str,
    capabilities: &str,
    input_json: Option<&str>,
    task_id: Option<&str>,
    explicit_worker: Option<&str>,
    evaluate: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use openaip_os::worker::orchestrator::WorkerOrchestrator;
    use openaip_os::worker::router::{RouterCriteria, WorkerRouter};

    let storage = openaip_os::storage::LibSQLStorage::open(db).await?;
    let input: serde_json::Value = match input_json {
        Some(s) => serde_json::from_str(s)?,
        None => serde_json::json!({}),
    };

    let caps: Vec<String> = capabilities
        .split(',')
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect();
    let (registry, _) = default_worker_env();
    let _ = storage.sync_workers(&registry).await;
    let policy = policy_for_capabilities(&caps);

    // Router Selection Learning（Trust 轴）：经验影响下一次选择（Policy Filter 之后）
    let learned = openaip_os::worker::stats::learned_trust_map(&storage).await?;
    let router = WorkerRouter::new(RouterCriteria {
        min_quality: 0.0,
        max_cost: u32::MAX,
        max_risk: 10,
        explicit: explicit_worker.map(String::from),
        learned_trust: learned,
    });

    let tasks: Vec<openaip_os::worker::orchestrator::WorkerTask> = capabilities
        .split(',')
        .map(|c| c.trim())
        .filter(|c| !c.is_empty())
        .map(|c| openaip_os::worker::orchestrator::WorkerTask::new(c, input.clone()))
        .collect();
    if tasks.is_empty() {
        println!("⚠️ 空计划：未指定 capability");
        return Ok(());
    }

    // 注册 run 记录（Loop Health closure_rate 的分母；评测在 run 尾部落库）
    let run_id = openaip_os::session::run::generate_run_id();
    let _ = storage
        .insert_run(&run_id, "worker-cli", capabilities)
        .await;

    let mut orch = WorkerOrchestrator::new(&registry, &policy, router);
    let result = orch.run_plan(&tasks).await;
    // Router Selection Learning：本次执行结果沉淀为 (capability, worker) 统计
    // （数据源：run 结束后评测结果中的 per-worker outcomes——可回放）
    println!(
        "🧠 Worker 计划执行：run_id={} all_ok={}",
        result.run_id, result.all_ok
    );
    for o in &result.outcomes {
        println!(
            "   [{}] {} → {:?} (worker={:?}, {:.2}s)",
            o.status.status_str(),
            o.capability,
            o.error.as_deref().unwrap_or(""),
            o.worker_id,
            o.duration_ms as f64 / 1000.0
        );
    }
    println!(
        "   success_rate={:.2} total_duration={:.2}s",
        result.success_rate(),
        result.total_duration_ms() as f64 / 1000.0
    );

    // Phase 11A：RunEnd → Evaluation（worker.* 真实事件 → 六维评测 → wb_evaluation）
    if evaluate {
        orch.session_log().append(
            openaip_os::session::event::EventType::SessionCompleted,
            serde_json::json!({ "run_id": result.run_id }),
        );
        let run_id = result.run_id.clone();
        let task = openaip_os::evaluation::TaskDefinition {
            task_id: task_id.unwrap_or(&run_id).to_string(),
            expected_relations: vec![],
            expected_artifacts: vec![],
            time_budget_secs: 0.0,
            skill_version: None,
        };
        match openaip_os::on_run_end(
            orch.session_log(),
            &run_id,
            &task,
            &openaip_os::EvaluationPolicy::default(),
            &storage,
        )
        .await
        {
            Ok(r) => {
                println!(
                    "📊 Phase 11 评测：aggregate={:.3} correctness={:?} efficiency={:?} reliability={:?} policy_compliance={:?}",
                    r.aggregate,
                    r.correctness.value,
                    r.efficiency.value,
                    r.reliability.value,
                    r.policy_compliance.value
                );
            }
            Err(e) => println!("⚠️ 评测失败（不影响运行结果）：{e}"),
        }
    }
    Ok(())
}
