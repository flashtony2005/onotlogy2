//! `oaip` —— OpenAIP-OS 命令行（V0.1：compile / run / object list / event list；V0.2：relation / event emit；V0.3：agent run / serve）

use clap::{Parser, Subcommand};
use openaip_os::auth::{IdentityProvider, LocalIdentityProvider};
use openaip_os::evaluation::hook::{append_bridged, on_run_end, trace_to_session_events};
use openaip_os::evaluation::{EvaluationPolicy, TaskDefinition};
use openaip_os::server;
use openaip_os::session::event::{EventType, SessionId};
use openaip_os::session::log::SessionLog;
use openaip_os::storage::{LibSQLStorage, ObjectEngine, RelationEngine, TraceQuery, UserRecord};
use openaip_os::{codegen, parse_odl};
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;

#[derive(Parser)]
#[command(name = "oaip", version, about = "OpenAIP-OS 可执行本体平台 CLI (V0.3)")]
struct Cli {
    #[arg(long, default_value = "openaip.db", global = true)]
    db: String,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 解析 .odl 并生成 Rust struct / schema.sql / ontology.json 到 ./gen/
    Compile { file: String },
    /// 演示：写入示例 Customer 并列出（对应 TC-006）
    Run,
    /// 对象相关命令
    Object {
        #[command(subcommand)]
        cmd: ObjectCmd,
    },
    /// 关系（World Model 要素二）
    Relation {
        #[command(subcommand)]
        cmd: RelationCmd,
    },
    /// 事件轨迹相关命令
    Event {
        #[command(subcommand)]
        cmd: EventCmd,
    },
    /// Agent 闭环（V0.3 = MVP）：Pi 作 Cognitive Engine，OpenAIP 拥有 Planner
    Agent {
        #[command(subcommand)]
        cmd: AgentCmd,
    },
    /// Worker ABI（Phase 11D）：Thinker 制定计划，Worker 执行——OpenAIP-OS 不亲自干活
    Worker {
        #[command(subcommand)]
        cmd: WorkerCmd,
    },
    /// Loop Health（V2 多速率环元评估）：四平面六指标
    Health,
    /// Decision Replay（S1）：用当前 Trust 重跑历史路由决策（Learning Counterfactual，Projection 不改变历史）
    Replay,
    /// Simulation（S2/S3）：基于当前世界状态做假设投影与反事实对比（Decision Support，永不改世界）
    Simulate {
        /// 假设动作 A（JSON：HypotheticalAction）
        #[arg(long)]
        a: String,
        /// 假设动作 B（JSON：HypotheticalAction）
        #[arg(long)]
        b: String,
        /// 动作 A 假设派给的 Worker id（codex / claude-code / dsh-harness / pi-agent / echo / git）
        #[arg(long)]
        worker_a: Option<String>,
        /// 动作 B 假设派给的 Worker id
        #[arg(long)]
        worker_b: Option<String>,
    },
    /// 启动 Agent 观察服务（HTTP + WebSocket + 三层观察 UI）
    Serve {
        #[arg(long, default_value_t = 8080)]
        port: u16,
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value = "ui/dist")]
        static_dir: String,
        /// ODL 文件：声明对象自有动作（V0.4 完整 Action Engine）；缺省用 examples/crm.odl。
        #[arg(long, default_value = "examples/crm.odl")]
        odl: Option<String>,
        /// 自动批准所有需审批动作（跳过 Human Approval）；默认 false → require_approval 动作被拒（WB2005）。
        #[arg(long, default_value_t = false)]
        auto_approve: bool,
        /// REQ-018 Evolution 第二阶段 AutoApprove：允许基于已验证 LearnedKnowledge 自动放行 require_approval 动作（替代一次 Human Approval）。默认 false → 等同 MVP。
        #[arg(long, default_value_t = false)]
        auto_approve_evolution: bool,
    },
    /// 数据库维护（schema 迁移 / 清理演示数据）
    Db {
        #[command(subcommand)]
        cmd: DbCmd,
    },
    /// 用户与账号管理（V0.7 认证）
    User {
        #[command(subcommand)]
        cmd: UserCmd,
    },
}

#[derive(Subcommand)]
enum AgentCmd {
    /// 触发 Agent 运行（Pi 作 Cognitive Engine）：oaip agent run "reduce churn"
    Run {
        /// 自然语言目标，经 Pi（Cognitive Engine）NLU 解析为 Intent
        goal: String,
        /// Agent ID
        #[arg(long, default_value = "retention-agent")]
        agent_id: String,
        /// ODL 文件：声明对象自有动作（V0.4 完整 Action Engine）；缺省用 examples/crm.odl。
        #[arg(long, default_value = "examples/crm.odl")]
        odl: Option<String>,
        /// 自动批准所有需审批动作（跳过交互式 Human Approval）；默认 false → 经 stdin 交互确认。
        #[arg(long, default_value_t = false)]
        auto_approve: bool,
        /// REQ-018 Evolution 第二阶段 AutoApprove：允许基于已验证 LearnedKnowledge 自动放行 require_approval 动作。默认 false → 等同 MVP。
        #[arg(long, default_value_t = false)]
        auto_approve_evolution: bool,
        /// V2 桥：计划经 Worker ABI 执行（Thinker 规划 → Worker 干活），而非 ActionEngine
        #[arg(long, default_value_t = false)]
        via_workers: bool,
        /// 执行所用 Skill 版本（写入 wb_evaluation.skill_version，learning_effectiveness 观测前提）
        #[arg(long)]
        skill: Option<String>,
    },
}

#[derive(Subcommand)]
enum WorkerCmd {
    /// 通过 Worker ABI 执行 capability 计划（Phase 11D Orchestrator）：
    /// oaip worker run code.test,code.build —— Thinker 制定计划，Worker 执行。
    Run {
        /// capability 计划（逗号分隔；每个 capability 经 Router → Policy → Worker）
        capabilities: String,
        /// 单步输入 JSON（应用到每一步）
        #[arg(long)]
        input: Option<String>,
        /// 任务 ID（评测聚合键，缺省用 run_id）
        #[arg(long)]
        task: Option<String>,
        /// 显式指定 Worker（Router explicit 判据，跳过评分）：
        /// codex / claude-code / dsh-harness / pi-agent / echo / git
        #[arg(long)]
        worker: Option<String>,
        /// 是否自动评测（on_run_end → wb_evaluation；默认 true）
        #[arg(long, default_value_t = true)]
        evaluate: bool,
    },
}

#[derive(Subcommand)]
enum ObjectCmd {
    /// 列出某类型的对象（渲染 `id name level`）
    List { object_type: String },
}

#[derive(Subcommand)]
enum RelationCmd {
    /// 建立关系边：oaip relation link <src> <edge> <dst>
    Link {
        source: String,
        edge: String,
        target: String,
    },
    /// 列出关系：--from <id> 或 --edge <type>
    List {
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        edge: Option<String>,
    },
}

#[derive(Subcommand)]
enum EventCmd {
    /// 列出 world 事件轨迹
    List,
    /// 发出领域业务事件：oaip event emit PaymentLate --refs 001,002
    Emit {
        event_type: String,
        #[arg(long, value_delimiter = ',')]
        refs: Vec<String>,
    },
}

#[derive(Subcommand)]
enum DbCmd {
    /// 删除并重建本地数据库（schema 迁移 / 清理演示数据）
    Reset,
}

#[derive(Subcommand)]
enum UserCmd {
    /// 列出所有用户账号（V0.7 认证支持）
    List,
    /// 离线创建/确保管理员账号（默认 Developer 角色，幂等）；用于首次部署或 env 不便时。
    CreateAdmin {
        username: String,
        /// 口令（建议经环境变量传入，避免出现在 shell 历史）。
        #[arg(long)]
        password: String,
    },
    /// 通用创建账号（默认 Developer 角色）
    Create {
        username: String,
        /// 口令
        #[arg(long)]
        password: String,
        #[arg(long, default_value = "Developer")]
        role: String,
        #[arg(long, default_value = "")]
        display_name: String,
    },
    /// 删除账号（幂等）
    Delete { username: String },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Compile { file } => cmd_compile(&file).await?,
        Cmd::Run => cmd_run(&cli.db).await?,
        Cmd::Object { cmd } => match cmd {
            ObjectCmd::List { object_type } => cmd_object_list(&cli.db, &object_type).await?,
        },
        Cmd::Relation { cmd } => match cmd {
            RelationCmd::Link {
                source,
                edge,
                target,
            } => cmd_relation_link(&cli.db, &source, &edge, &target).await?,
            RelationCmd::List { from, edge } => cmd_relation_list(&cli.db, from, edge).await?,
        },
        Cmd::Event { cmd } => match cmd {
            EventCmd::List => cmd_event_list(&cli.db).await?,
            EventCmd::Emit { event_type, refs } => {
                cmd_event_emit(&cli.db, &event_type, &refs).await?
            }
        },
        Cmd::Agent { cmd } => match cmd {
            AgentCmd::Run {
                goal,
                agent_id,
                odl,
                auto_approve,
                auto_approve_evolution,
                via_workers,
                skill,
            } => {
                cmd_agent_run(
                    &cli.db,
                    &goal,
                    &agent_id,
                    odl.as_deref(),
                    auto_approve,
                    auto_approve_evolution,
                    via_workers,
                    skill.as_deref(),
                )
                .await?
            }
        },
        Cmd::Worker { cmd } => match cmd {
            WorkerCmd::Run {
                capabilities,
                input,
                task,
                worker,
                evaluate,
            } => {
                cmd_worker_run(
                    &cli.db,
                    &capabilities,
                    input.as_deref(),
                    task.as_deref(),
                    worker.as_deref(),
                    evaluate,
                )
                .await?
            }
        },
        Cmd::Health => cmd_health(&cli.db).await?,
        Cmd::Replay => cmd_replay(&cli.db).await?,
        Cmd::Simulate {
            a,
            b,
            worker_a,
            worker_b,
        } => cmd_simulate(&cli.db, &a, &b, worker_a.as_deref(), worker_b.as_deref()).await?,
        Cmd::Serve {
            port,
            host,
            static_dir,
            odl,
            auto_approve,
            auto_approve_evolution,
        } => {
            cmd_serve(
                &cli.db,
                &host,
                port,
                &static_dir,
                odl.as_deref(),
                auto_approve,
                auto_approve_evolution,
            )
            .await?
        }
        Cmd::Db { cmd } => match cmd {
            DbCmd::Reset => cmd_db_reset(&cli.db).await?,
        },
        Cmd::User { cmd } => match cmd {
            UserCmd::List => cmd_user_list(&cli.db).await?,
            UserCmd::CreateAdmin { username, password } => {
                cmd_user_create_admin(&cli.db, &username, &password).await?
            }
            UserCmd::Create {
                username,
                password,
                role,
                display_name,
            } => cmd_user_create(&cli.db, &username, &password, &role, &display_name).await?,
            UserCmd::Delete { username } => cmd_user_delete(&cli.db, &username).await?,
        },
    }
    Ok(())
}

async fn cmd_compile(file: &str) -> Result<(), Box<dyn std::error::Error>> {
    let src = std::fs::read_to_string(file)?;
    let ir = parse_odl(&src)?;
    let out = codegen::codegen(&ir)?;
    std::fs::create_dir_all("./gen")?;
    std::fs::write("./gen/ontology.rs", &out.rust)?;
    std::fs::write("./gen/schema.sql", &out.schema_sql)?;
    std::fs::write("./gen/ontology.json", &out.ontology_json)?;
    println!(
        "✅ 编译完成：{} 个对象类型 → ./gen/ (ontology.rs, schema.sql, ontology.json)",
        ir.object_types.len()
    );
    Ok(())
}

async fn cmd_run(db: &str) -> Result<(), Box<dyn std::error::Error>> {
    let storage = LibSQLStorage::open(db).await?;
    // 演示数据（对应 TC-006）：硬删除旧演示行后重灌，保证可重复运行
    storage.hard_delete("001").await?;
    storage.hard_delete("002").await?;
    storage
        .create("Customer", "001", json!({"name": "Apple", "level": "VIP"}))
        .await?;
    storage
        .create("Customer", "002", json!({"name": "Tesla", "level": "GOLD"}))
        .await?;
    let recs = storage.query("Customer").await?;
    println!("{}", LibSQLStorage::render_object_list(&recs));
    Ok(())
}

async fn cmd_object_list(db: &str, object_type: &str) -> Result<(), Box<dyn std::error::Error>> {
    let storage = LibSQLStorage::open(db).await?;
    let recs = storage.query(object_type).await?;
    if recs.is_empty() {
        println!("(无 {object_type} 对象，先运行 `oaip run`)");
    } else {
        println!("{}", LibSQLStorage::render_object_list(&recs));
    }
    Ok(())
}

async fn cmd_event_list(db: &str) -> Result<(), Box<dyn std::error::Error>> {
    let storage = LibSQLStorage::open(db).await?;
    let events = storage.list_world_events().await?;
    if events.is_empty() {
        println!("(无 world 事件，先运行 `oaip run`)");
    } else {
        for e in events {
            println!(
                "{} | {} | {} | refs={:?}",
                e.timestamp, e.event_type, e.id, e.object_refs
            );
        }
    }
    Ok(())
}

async fn cmd_relation_link(
    db: &str,
    source: &str,
    edge: &str,
    target: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let storage = LibSQLStorage::open(db).await?;
    match storage.link(source, edge, target, None).await {
        Ok(rec) => println!(
            "✅ 已建立关系 {}: {} -{}-> {}",
            rec.id, source, edge, target
        ),
        Err(e) => println!("❌ 建立关系失败：{e}"),
    }
    Ok(())
}

async fn cmd_relation_list(
    db: &str,
    from: Option<String>,
    edge: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let storage = LibSQLStorage::open(db).await?;
    let edges = match (from, edge) {
        (Some(f), _) => storage.neighbors(&f).await?,
        (None, Some(e)) => storage.query_by_edge(&e).await?,
        (None, None) => {
            println!("(需 --from <id> 或 --edge <type> 之一)");
            return Ok(());
        }
    };
    if edges.is_empty() {
        println!("(无关系边)");
    } else {
        for r in edges {
            println!("{} | {} -{}-> {}", r.id, r.source, r.relation, r.target);
        }
    }
    Ok(())
}

async fn cmd_event_emit(
    db: &str,
    event_type: &str,
    refs: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let storage = LibSQLStorage::open(db).await?;
    let ev = openaip_os::trace::TraceEvent::world(
        event_type,
        refs.first().map(String::as_str).unwrap_or("world"),
    );
    match storage.append_trace(&ev).await {
        Ok(()) => println!("✅ 已发出领域事件 {}，绑定 refs={:?}", event_type, refs),
        Err(e) => println!("❌ 发出领域事件失败：{e}"),
    }
    Ok(())
}

async fn cmd_db_reset(db: &str) -> Result<(), Box<dyn std::error::Error>> {
    // 删除库文件及其 WAL/SHM 伴生文件，再由 open 以 canonical_schema 重建新表。
    for suffix in ["", "-wal", "-shm"] {
        let p = format!("{db}{suffix}");
        if std::path::Path::new(&p).exists() {
            std::fs::remove_file(&p)?;
        }
    }
    LibSQLStorage::open(db).await?;
    println!("✅ 已重建数据库 {db}（新 schema：type / source / relation / target）");
    Ok(())
}

/// 触发 Agent 运行（M1c · V2）：PiEngine 作 Cognitive Engine，AgentLoop 闭环。
/// `via_workers=true` 时走 V2 桥：Thinker 决策 → Worker ABI 执行。
#[allow(clippy::too_many_arguments)]
async fn cmd_agent_run(
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
    use openaip_os::evolution2::{EvolutionEngine, EvolutionSnapshot};
    use openaip_os::kernel::{Kernel, Scope};
    use openaip_os::ontology::ir_to_services;
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
    let mut kernel = Kernel::new();
    let mut agent = Agent::new(
        agent_id,
        kernel.create_context(None, Scope::World, "world")?,
    );
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

    let report = run_agent(
        &mut kernel,
        &mut agent,
        &services,
        &engine,
        Some(openaip_os::ontology::ir_to_runtime(&ir)),
        evolution.as_mut().map(|(ev, _)| ev),
        &run_id,
        goal,
        &mut trace,
        &mut approvals,
        &mut resolver,
    )?;

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

/// 预置 Worker 环境（能力市场）：echo / git 真实 + codex / claude / dsh / pi 适配器。
/// Policy：fail-closed（未配置 = DENY）；shell.execute / code.write 需审批——Worker ≠ Authority。
fn default_worker_env() -> (
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
fn policy_for_capabilities(caps: &[String]) -> openaip_os::worker::policy::WorkerPolicy {
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
async fn cmd_worker_run(
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

/// Decision Replay（S1）：当前 Trust 重跑历史路由决策（Projection，永不改历史）。
async fn cmd_replay(db: &str) -> Result<(), Box<dyn std::error::Error>> {
    let storage = LibSQLStorage::open(db).await?;
    let report = openaip_os::simulation::replay::run_decision_replay(&storage).await?;
    println!(
        "🔁 Decision Replay（Learning Counterfactual）：{} 条决策，{} 条会因当前经验改变（{:.0}%）",
        report.total_decisions,
        report.changed,
        report.change_rate * 100.0
    );
    for d in &report.decisions {
        if d.changed {
            println!(
                "   ⚡ run={} capability={} actual={} → replay={}",
                d.run_id, d.capability, d.actual_worker, d.replay_worker
            );
        }
    }
    if report.decisions.is_empty() {
        println!("   （无历史决策数据——先跑几次 oaip worker run 积累经验）");
    }
    Ok(())
}

/// Simulation（S2/S3）：当前世界状态 + 假设动作 → 投影对比 → 决策支撑。
/// 铁律：产出是 Projection（假设轨），永不写入 EventLog（I2）。
async fn cmd_simulate(
    db: &str,
    a_json: &str,
    b_json: &str,
    worker_a: Option<&str>,
    worker_b: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    use openaip_os::simulation::projection::HypotheticalAction;

    let storage = LibSQLStorage::open(db).await?;
    let a: HypotheticalAction = serde_json::from_str(a_json)?;
    let mut b: HypotheticalAction = serde_json::from_str(b_json)?;
    if let Some(w) = worker_a {
        // worker_id 是动作的一部分（S3 worker 维度）；从参数注入
        let mut a = a;
        a.worker_id = Some(w.to_string());
        return simulate_inner(&storage, &a, &mut b, worker_b).await;
    }
    let _ = &mut b;
    simulate_inner(&storage, &a, &mut b, worker_b).await
}

async fn simulate_inner(
    storage: &LibSQLStorage,
    a: &openaip_os::simulation::projection::HypotheticalAction,
    b: &mut openaip_os::simulation::projection::HypotheticalAction,
    worker_b: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    use openaip_os::simulation::projection::{counterfactual, project};
    use openaip_os::worker::policy::WorkerPolicy;

    if let Some(w) = worker_b {
        b.worker_id = Some(w.to_string());
    }

    // 状态：真实世界关系集（Decision Support 基于当前世界）
    let rels = storage.list_relations().await?;
    let state: Vec<(String, String, String)> = rels
        .iter()
        .map(|r| (r.source.clone(), r.relation.clone(), r.target.clone()))
        .collect();
    println!("🌍 当前世界状态：{} 条关系", state.len());
    for (s, p, t) in &state {
        println!("   {s} -[{p}]-> {t}");
    }

    // Policy（与 worker run 一致：shell.execute/code.write 需审批，其余 allow）
    let mut policy = WorkerPolicy::new();
    for cap in [&a.capability, &b.capability].into_iter().flatten() {
        policy = match cap.as_str() {
            "shell.execute" | "code.write" => policy.require_approval(cap),
            _ => policy.allow(cap),
        };
    }

    // Worker 信任维度（诚实边界）：动作级模拟用 declared trust——learned 是 (capability, worker)
    // 级修正，作用域是路由决策（S1 replay 已覆盖）；此处不跨作用域借用。
    let manifests = storage.list_workers().await?;
    let mut worker_scores: Vec<(String, f64)> =
        manifests.iter().map(|m| (m.id.clone(), m.trust)).collect();
    worker_scores.sort_by(|x, y| y.1.partial_cmp(&x.1).unwrap_or(std::cmp::Ordering::Equal));
    if let Some(w) = &b.worker_id
        && !worker_scores.iter().any(|(id, _)| id == w)
    {
        eprintln!(
            "⚠️ 未知 Worker id「{w}」——可用：{}",
            worker_scores
                .iter()
                .map(|(id, _)| id.as_str())
                .collect::<Vec<_>>()
                .join(" / ")
        );
    }

    // S2：两投影
    let pa = project(&state, a);
    let pb = project(&state, b);
    println!("🔮 S2 投影（假设轨，永不写入事件源）");
    println!(
        "   A「{}」→ 新增 {} 条关系（可逆性 {}）",
        a.action,
        pa.predicted_state["added"]
            .as_array()
            .map(|x| x.len())
            .unwrap_or(0),
        a.reversibility
    );
    println!(
        "   B「{}」→ 新增 {} 条关系（可逆性 {}）",
        b.action,
        pb.predicted_state["added"]
            .as_array()
            .map(|x| x.len())
            .unwrap_or(0),
        b.reversibility
    );

    // S3：反事实对比
    let report = counterfactual(&state, a, b, &policy, &worker_scores);
    println!("⚖️ S3 反事实对比");
    for d in &report.dimensions {
        println!(
            "   「{}」outcome={} risk={} policy={} worker_trust={:?} → {}",
            d.action,
            d.outcome_added,
            d.risk_level,
            d.policy,
            d.worker_trust,
            if d.recommended {
                "✓ 可执行"
            } else {
                "✗ 不可自动执行"
            }
        );
    }
    match &report.recommendation {
        Some(r) => println!("🎯 推荐：{r}（确定性规则：Policy 允许 → 低风险 → worker 信任）"),
        None => println!("🎯 无推荐（双方均不可自动执行——Simulation 不能绕过 Governance）"),
    }
    Ok(())
}

/// Loop Health：从持久化数据派生四平面六指标（确定性、可回放）。
async fn cmd_health(db: &str) -> Result<(), Box<dyn std::error::Error>> {
    let storage = LibSQLStorage::open(db).await?;
    let h = openaip_os::health::collect(&storage).await?;
    println!("🩺 Loop Health（V2 多速率环元评估）");
    println!(
        "   EXECUTION   closure_rate={:.2} action_success={:.2} world_change={:.2} (runs={})",
        h.execution.closure_rate,
        h.execution.action_success_rate,
        h.execution.world_change_rate,
        h.execution.total_runs
    );
    println!(
        "   OBSERVATION evaluation_coverage={:.2} (evaluated_runs={})",
        h.observation.evaluation_coverage, h.observation.evaluated_runs
    );
    println!(
        "   EVOLUTION   mutation_consumption={:.2} learning_effectiveness={:+.3} regression={:.2} (candidates={})",
        h.evolution.mutation_consumption,
        h.evolution.learning_effectiveness,
        h.evolution.regression_rate,
        h.evolution.experimental_or_active
    );
    println!(
        "   GOVERNANCE  violation_rate={:.2} (policy_checks={})",
        h.governance.violation_rate, h.governance.policy_checks
    );
    Ok(())
}

/// 启动 Agent 观察服务（HTTP + WebSocket + 三层观察 UI）。
async fn cmd_serve(
    db: &str,
    host: &str,
    port: u16,
    static_dir: &str,
    odl: Option<&str>,
    auto_approve: bool,
    auto_approve_evolution: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let storage = LibSQLStorage::open(db).await?;
    // V0.7 认证：空库必须先用 env 或 `oaip user create-admin` 建管理员，否则拒绝启动。
    let idp = LocalIdentityProvider::new(storage.clone());
    let has_users = storage.count_users().await? > 0;
    if !has_users {
        match (
            std::env::var("WB_BOOTSTRAP_ADMIN_USER"),
            std::env::var("WB_BOOTSTRAP_ADMIN_PWD"),
        ) {
            (Ok(u), Ok(p)) => idp.bootstrap_admin(&u, &p).await?,
            _ => {
                return Err(
                    "未配置管理员账号：请设置环境变量 WB_BOOTSTRAP_ADMIN_USER / WB_BOOTSTRAP_ADMIN_PWD，\
                     或先运行 `oaip user create-admin <用户名> --password <口令>` 建账号"
                        .into(),
                )
            }
        }
    }
    let storage = Arc::new(storage);
    // M1c V2 run 路径：ODL → compose::Service 集合 + PiEngine（V2 认知引擎）。
    let odl_src = std::fs::read_to_string(odl.unwrap_or("examples/crm.odl"))?;
    let odl_ir = parse_odl(&odl_src)?;
    let services = openaip_os::ontology::ir_to_services(&odl_ir);
    let effect_defs = openaip_os::ontology::ir_to_runtime(&odl_ir);
    let engine: Arc<dyn openaip_os::cognitive::CognitiveEngine> =
        Arc::new(openaip_os::cognitive::PiEngine::new("retention-agent"));
    let addr: SocketAddr = format!("{host}:{port}").parse()?;
    let listener = TcpListener::bind(addr).await?;
    println!(
        "🚀 OpenAIP-OS Agent Observation 服务已启动：\n  UI  → http://{addr}/\n  API → http://{addr}/api/v1/wb/agent\n  WS  → ws://{addr}/api/v1/wb/agent/{{runId}}/stream\n  认证 → 已强制（V0.7）；先 POST /api/v1/wb/auth/login 取令牌"
    );
    server::serve(
        storage,
        services,
        effect_defs,
        engine,
        listener,
        static_dir,
        auto_approve,
        auto_approve_evolution,
        true,
    )
    .await?;
    Ok(())
}

/// 离线创建/确保管理员账号（默认 Developer，幂等）。
async fn cmd_user_create_admin(
    db: &str,
    username: &str,
    password: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let storage = LibSQLStorage::open(db).await?;
    let idp = LocalIdentityProvider::new(storage);
    idp.bootstrap_admin(username, password).await?;
    println!("✅ 管理员账号已就绪：{username}（角色 Developer）");
    Ok(())
}

/// 通用创建账号（默认 Developer 角色）
async fn cmd_user_create(
    db: &str,
    username: &str,
    password: &str,
    role: &str,
    display_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let role_str = role.to_lowercase();
    // 角色白名单校验：非法角色会让该用户登录时永久失败（Role::from_str 报错），必须前置拦截。
    if !matches!(role_str.as_str(), "executive" | "operator" | "developer") {
        return Err(format!("未知角色: {role}（可选 executive / operator / developer）").into());
    }
    let storage = LibSQLStorage::open(db).await?;
    let rec = UserRecord {
        id: format!("user.{username}"),
        username: username.to_string(),
        password_hash: LocalIdentityProvider::hash_password(password)?,
        role: role_str,
        display_name: display_name.to_string(),
        disabled: false,
    };
    storage.create_user(&rec).await?;
    println!("✅ 用户账号已创建：{username}（角色 {role}）");
    Ok(())
}

/// 列出所有用户账号
async fn cmd_user_list(db: &str) -> Result<(), Box<dyn std::error::Error>> {
    let storage = LibSQLStorage::open(db).await?;
    let users = storage.list_users().await?;
    if users.is_empty() {
        println!("（无用户账号，先运行 `oaip user create-admin <用户名> --password <口令>`）");
        return Ok(());
    }
    for u in users {
        println!(
            "{} | {} | {} | disabled={}",
            u.username, u.role, u.display_name, u.disabled
        );
    }
    Ok(())
}

/// 删除账号（幂等）
async fn cmd_user_delete(db: &str, username: &str) -> Result<(), Box<dyn std::error::Error>> {
    let storage = LibSQLStorage::open(db).await?;
    storage.delete_user(username).await?;
    println!("✅ 用户账号已删除：{username}");
    Ok(())
}
