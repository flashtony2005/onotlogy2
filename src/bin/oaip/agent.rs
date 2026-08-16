//! agent run（M5 拆分）：V2 认知闭环 + via_workers 桥。

use super::worker::{default_worker_env, policy_for_capabilities};
use openaip_os::abi::odl::parse_odl;
use openaip_os::evaluation::hook::{append_bridged, on_run_end, trace_to_session_events};
use openaip_os::evaluation::{EvaluationPolicy, TaskDefinition};
use openaip_os::session::event::{EventType, SessionId};
use openaip_os::session::log::SessionLog;
use openaip_os::storage::{LibSQLStorage, TraceQuery};
use serde_json::json;

#[allow(clippy::too_many_arguments)]
pub async fn cmd_agent_run(
    db: &str,
    goal: &str,
    agent_id: &str,
    odl: Option<&str>,
    auto_approve: bool,
    auto_approve_evolution: bool,
    via_workers: bool,
    skill: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    use openaip_os::cognitive::approval::{ApprovalDecision, ApprovalRequest, DecisionCache};
    use openaip_os::cognitive::persist::CognitiveStore;
    use openaip_os::cognitive::runner::run_agent;
    use openaip_os::cognitive::{Agent, CognitiveEngine, LoopOutcome, PiEngine};
    use openaip_os::compose::defs::ir_to_services;
    use openaip_os::evolution2::{EvolutionEngine, EvolutionSnapshot};
    use openaip_os::kernel::{Kernel, Scope};
    use openaip_os::session::run::generate_run_id;
    use openaip_os::worker::thinker::Thinker;

    let storage = LibSQLStorage::open(db).await?;
    let path = odl.unwrap_or("examples/crm.odl");
    let src = std::fs::read_to_string(path)?;
    let ir = parse_odl(&src)?;
    let services = ir_to_services(&ir);
    let engine = PiEngine::new(agent_id.to_string());

    if !engine.health() {
        println!(
            "❌ WB2004 认知引擎不可用：未找到 `pi` 可执行文件。请先安装 @earendil-works/pi-coding-agent 并配置 LLM Provider。"
        );
        return Ok(());
    }

    let run_id = generate_run_id();
    storage.insert_run(&run_id, agent_id, goal).await?;
    // Phase 11 11A：Run 级 SessionLog（事实源）。Context Assembly 事实先落。
    let mut log = SessionLog::new(SessionId::new(&run_id));
    log.append(
        EventType::ContextAssembled,
        json!({
            "run_id": run_id,
            "assets": [
                { "asset_id": "goal", "selection_reason": goal, "relevance": 1.0 },
                { "asset_id": "agent", "selection_reason": agent_id, "relevance": 1.0 },
                { "asset_id": "ontology", "selection_reason": path, "relevance": 1.0 },
            ],
        }),
    );

    if via_workers {
        // V2 桥：Thinker（CognitiveEngine::decide）→ Worker ABI 执行（Worker ≠ Authority）
        let caps: Vec<String> = services
            .iter()
            .flat_map(|s| s.capability_names().cloned())
            .collect();
        let decisions = Thinker::new(&engine).plan(goal, &caps, vec![]);
        let tasks = Thinker::to_tasks(&decisions);
        println!("🧠 Thinker 决策：{} 步", tasks.len());
        for t in &tasks {
            println!("   - {} ({})", t.capability, t.note);
        }
        let (wregistry, _) = default_worker_env();
        let wpolicy = policy_for_capabilities(&caps);
        let mut orch = openaip_os::worker::orchestrator::WorkerOrchestrator::new(
            &wregistry,
            &wpolicy,
            openaip_os::worker::router::WorkerRouter::new(
                openaip_os::worker::router::RouterCriteria::default(),
            ),
        );
        let result = orch.run_plan(&tasks).await;
        println!(
            "🧠 Worker 计划执行：run_id={} all_ok={}",
            result.run_id, result.all_ok
        );
        for o in &result.outcomes {
            println!(
                "   [{}] {} → {:?} (worker={:?})",
                o.status.status_str(),
                o.capability,
                o.error.as_deref().unwrap_or(""),
                o.worker_id
            );
        }
        let ok = result.all_ok;
        orch.session_log()
            .append(EventType::SessionCompleted, json!({ "run_id": run_id }));
        let task = TaskDefinition {
            task_id: goal.to_string(),
            expected_relations: vec![],
            expected_artifacts: vec![],
            time_budget_secs: 0.0,
            skill_version: skill
                .map(String::from)
                .or_else(|| Some(agent_id.to_string())),
        };
        match on_run_end(
            orch.session_log(),
            &run_id,
            &task,
            &EvaluationPolicy::default(),
            &storage,
        )
        .await
        {
            Ok(r) => {
                let _ =
                    openaip_os::worker::stats::record_outcomes(&storage, &r.worker_outcomes).await;
                println!(
                    "📊 Phase 11 评测：aggregate={:.3} skill_version={:?} reliability={:?} policy_compliance={:?}",
                    r.aggregate, r.skill_version, r.reliability.value, r.policy_compliance.value
                );
            }
            Err(e) => println!("⚠️ 评测失败（不影响运行结果）：{e}"),
        }
        storage
            .update_run_status(
                &run_id,
                if ok { "Completed" } else { "Failed" },
                Some(&openaip_os::trace::now_ts()),
            )
            .await?;
        return Ok(());
    }

    // ── V2 认知闭环：Kernel + Agent + AgentLoop（M1c 切换，planner 不再参与）──
    // P5.6：审计事件产生时即落库（kernel audit sink；不依赖 run 终局桥接）。
    let mut kernel = Kernel::new().with_audit_sink(std::sync::Arc::new(
        openaip_os::cognitive::runner::StorageAuditSink::new(
            std::sync::Arc::new(storage.clone()),
            tokio::runtime::Handle::current(),
            &run_id,
            agent_id,
        ),
    ));
    let ctx = kernel.create_context(None, Scope::World, "world")?;
    let mut agent = Agent::new(agent_id, ctx);

    // ── P5.4：per-run 世界投影——Storage 事件日志 → Kernel（Agent 首次观察即见 Storage 世界）──
    // 投影与 Agent 共享同一世界 Context；契约 P5：事件是世界唯一入口（无裸写）。
    let mut projection = openaip_os::Projection::new(ctx);
    let projected = projection
        .project_from_storage(&mut kernel, &storage)
        .await?;
    println!("🌍 P5.4 世界投影：{} 事件 → Kernel（观察就绪）", projected);
    let mut trace: Vec<openaip_os::trace::TraceEvent> = Vec::new();
    let mut approvals: Vec<ApprovalRequest> = Vec::new();

    // 审批裁决：--auto-approve 直放；否则 stdin 交互（同 action@target 缓存一次决策）。
    let mut cache = DecisionCache::new();
    let mut resolver = |req: &ApprovalRequest| -> ApprovalDecision {
        if auto_approve {
            return ApprovalDecision::Approve {
                by: "cli-auto".into(),
            };
        }
        cache.get_or_ask(&DecisionCache::key(&req.action, &req.args), || {
            println!("⚠️ 需要人工审批：{} args={}", req.capability, req.args);
            print!("  批准? [y/N]: ");
            use std::io::Write;
            let _ = std::io::stdout().flush();
            let mut line = String::new();
            let ok = std::io::stdin().read_line(&mut line).is_ok()
                && matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes");
            if ok {
                ApprovalDecision::Approve {
                    by: "cli-user".into(),
                }
            } else {
                ApprovalDecision::Reject {
                    by: "cli-user".into(),
                }
            }
        })
    };

    // 演化引擎：--auto-approve-evolution 开启 Gate；快照经 CognitiveStore 跨 run 持久化。
    let mut evolution: Option<(EvolutionEngine, Option<CognitiveStore>)> = None;
    if auto_approve_evolution {
        let store = CognitiveStore::open(db).await.ok();
        let restored = match &store {
            Some(s) => s
                .load::<EvolutionSnapshot>("evolution")
                .await
                .ok()
                .flatten()
                .map(EvolutionEngine::restore),
            None => None,
        };
        evolution = Some((
            restored
                .unwrap_or_else(EvolutionEngine::new)
                .with_gate_enabled(true),
            store,
        ));
    }

    // ── P5.5/P6：Effect 提交点世界事件同步落库（步骤级一致）──
    // block_in_place：同步闭环内 sink 用 Handle::block_on 驱动 storage append
    //（multi_thread runtime 下合法；pi 子进程/stdin 已在此同步路径内）。
    let world_sink: Option<std::sync::Arc<dyn openaip_os::compose::task::WorldEventSink>> = Some(
        std::sync::Arc::new(openaip_os::cognitive::runner::StorageWorldSink::new(
            std::sync::Arc::new(storage.clone()),
            tokio::runtime::Handle::current(),
        )),
    );

    let report = tokio::task::block_in_place(|| {
        run_agent(
            &mut kernel,
            &mut agent,
            &services,
            &engine,
            Some(openaip_os::compose::defs::ir_to_runtime(&ir)),
            evolution.as_mut().map(|(ev, _)| ev),
            &run_id,
            goal,
            &mut trace,
            &mut approvals,
            &mut resolver,
            world_sink,
        )
    })?;

    // 观察层落库：V2 事件流 → wb_trace_event（UI 时间线 / WS 同源）
    for e in &trace {
        storage.append_trace(e).await?;
    }
    if let Some((ev, Some(store))) = &evolution {
        let _ = store.save("evolution", &ev.snapshot()).await;
    }
    storage
        .update_run_status(
            &run_id,
            report.status.as_str(),
            Some(&openaip_os::trace::now_ts()),
        )
        .await?;

    match &report.outcome {
        LoopOutcome::Completed => println!(
            "✅ Agent 运行完成：run_id={} status={}",
            run_id,
            report.status.as_str()
        ),
        LoopOutcome::Denied(reason) => {
            println!("❌ WB2005 Policy 拒绝（需 Human Approval）：{reason}");
        }
        LoopOutcome::Failed(reason) => println!("❌ 运行失败：{reason}"),
        LoopOutcome::Waiting | LoopOutcome::Continue => unreachable!("runner 已收敛"),
    }
    for e in &trace {
        println!(
            "  [{}] refs={:?} conf={:?} — {}",
            e.event_type,
            e.object_refs,
            e.confidence,
            e.reasoning.clone().unwrap_or_default()
        );
    }

    // Phase 11 11A：RunEnd → Evaluation（成败皆评）。
    let stored_trace = storage.list_events(TraceQuery::for_run(&run_id)).await?;
    append_bridged(&mut log, trace_to_session_events(&stored_trace));
    log.append(EventType::SessionCompleted, json!({ "run_id": run_id }));
    let task = TaskDefinition {
        task_id: goal.to_string(),
        expected_relations: vec![],
        expected_artifacts: vec![],
        time_budget_secs: 0.0,
        skill_version: None,
    };
    match on_run_end(
        &mut log,
        &run_id,
        &task,
        &EvaluationPolicy::default(),
        &storage,
    )
    .await
    {
        Ok(r) => {
            let _ = openaip_os::worker::stats::record_outcomes(&storage, &r.worker_outcomes).await;
            println!(
                "📊 Phase 11 评测（wb_evaluation）：aggregate={:.3} unknowns={:?}",
                r.aggregate,
                r.unknowns()
            );
        }
        Err(e) => println!("⚠️ 评测失败（不影响运行结果）：{e}"),
    }
    // Phase 11B 诊断环：失败模式 → Mutation Proposal（等待人工评审，Proposal ≠ Mutation）
    match openaip_os::run_diagnosis(
        &storage,
        agent_id,
        "v2",
        Some(goal),
        &openaip_os::DiagnosisConfig::default(),
    )
    .await
    {
        Ok(proposals) if !proposals.is_empty() => {
            println!(
                "🔬 Phase 11 诊断（新提案 {} 条，待人工评审）：",
                proposals.len()
            );
            for p in &proposals {
                println!("   {} → {}（{}）", p.id, p.root_cause, p.candidate_version);
            }
        }
        Ok(_) => {}
        Err(e) => println!("⚠️ 诊断失败（不影响运行结果）：{e}"),
    }
    Ok(())
}
