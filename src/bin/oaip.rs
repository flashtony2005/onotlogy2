//! `oaip` —— OpenAIP-OS 命令行（V0.1：compile / run / object list / event list；V0.2：relation / event emit；V0.3：agent run / serve）

use clap::{Parser, Subcommand};
use openaip_os::action::ActionRegistry;
use openaip_os::agent::{AgentEngine, PiAgentEngine};
use openaip_os::auth::{IdentityProvider, LocalIdentityProvider};
use openaip_os::evaluation::hook::{append_bridged, on_run_end, trace_to_session_events};
use openaip_os::evaluation::{EvaluationPolicy, TaskDefinition};
use openaip_os::planner::{
    AutoApprover, ConsoleApprover, Planner, PlannerError, PolicyApprover, RuntimeConfig,
    generate_run_id,
};
use openaip_os::runtime::RuntimeKernel;
use openaip_os::server;
use openaip_os::session::event::{EventType, SessionId};
use openaip_os::session::log::SessionLog;
use openaip_os::storage::{LibSQLStorage, ObjectEngine, TraceQuery, UserRecord};
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
    let kernel = RuntimeKernel::new(storage);
    kernel
        .create_object("Customer", "001", json!({"name": "Apple", "level": "VIP"}))
        .await?;
    kernel
        .create_object("Customer", "002", json!({"name": "Tesla", "level": "GOLD"}))
        .await?;
    let recs = kernel.query_objects("Customer").await?;
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
    let kernel = RuntimeKernel::new(storage);
    match kernel.link_relation(source, edge, target).await {
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
    let kernel = RuntimeKernel::new(storage);
    let edges = match (from, edge) {
        (Some(f), _) => kernel.neighbors(&f).await?,
        (None, Some(e)) => kernel.query_relations(&e).await?,
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
    let kernel = RuntimeKernel::new(storage);
    let ref_strs: Vec<&str> = refs.iter().map(|s| s.as_str()).collect();
    match kernel.emit_domain_event(event_type, &ref_strs).await {
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

/// 触发 Agent 运行（V0.3）：Pi 作 Cognitive Engine，OpenAIP Planner 闭环。
/// `via_workers=true` 时走 V2 桥：Thinker 规划（plan()）→ Worker ABI 执行。
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
    let storage = LibSQLStorage::open(db).await?;
    let kernel = RuntimeKernel::new(storage);
    let engine = PiAgentEngine::new(agent_id.to_string());
    let action_registry = load_registry(odl)?;

    if !engine.health().await {
        println!(
            "❌ WB2004 认知引擎不可用：未找到 `pi` 可执行文件。请先安装 @earendil-works/pi-coding-agent 并配置 LLM Provider。"
        );
        return Ok(());
    }

    let run_id = generate_run_id();
    // Phase 11 11A：Run 级 SessionLog（事实源）。Context Assembly 事实先落
    // （run 级上下文输入；planner 内部装配的细化发射为下一迭代项）。
    let mut log = SessionLog::new(SessionId::new(&run_id));
    log.append(
        EventType::ContextAssembled,
        json!({
            "run_id": run_id,
            "assets": [
                { "asset_id": "goal", "selection_reason": goal, "relevance": 1.0 },
                { "asset_id": "agent", "selection_reason": agent_id, "relevance": 1.0 },
                { "asset_id": "ontology", "selection_reason": odl.unwrap_or(""), "relevance": 1.0 },
            ],
        }),
    );

    // V2 桥：Thinker 规划 → Worker 执行（Worker ≠ Authority：Policy 由 worker 层 fail-closed 治理）
    if via_workers {
        use openaip_os::planner::PlanStep;
        use openaip_os::worker::orchestrator::{WorkerOrchestrator, WorkerTask};
        use openaip_os::worker::router::{RouterCriteria, WorkerRouter};

        let (wregistry, _) = default_worker_env();
        let planner = Planner::with_actions(&kernel, action_registry);
        let planned = planner
            .plan(&engine, goal, agent_id, &run_id)
            .await
            .map_err(|e| format!("规划失败: {e}"))?;
        let caps: Vec<String> = planned
            .steps
            .iter()
            .filter_map(PlanStep::worker_capability)
            .collect();
        let wpolicy = policy_for_capabilities(&caps);
        println!(
            "🧠 Thinker 规划：{} 个步骤（{} 个经 Worker ABI）",
            planned.steps.len(),
            caps.len()
        );
        for s in &planned.steps {
            match s.worker_capability() {
                Some(cap) => println!("   - {cap} ({})", s.description),
                None => println!(
                    "   - ⏭️ kernel 内部操作（不经 Worker ABI）：{}",
                    s.description
                ),
            }
        }
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
        let mut orch = WorkerOrchestrator::new(
            &wregistry,
            &wpolicy,
            WorkerRouter::new(RouterCriteria::default()),
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
            &kernel.storage(),
        )
        .await
        {
            Ok(r) => {
                println!(
                    "📊 Phase 11 评测：aggregate={:.3} skill_version={:?} reliability={:?} policy_compliance={:?}",
                    r.aggregate, r.skill_version, r.reliability.value, r.policy_compliance.value
                );
            }
            Err(e) => println!("⚠️ 评测失败（不影响运行结果）：{e}"),
        }
        return Ok(());
    }

    // Human Approval 闸门：默认 ConsoleApprover（stdin 交互确认）；--auto-approve 直接放行。
    let approver: Arc<dyn PolicyApprover> = if auto_approve {
        Arc::new(AutoApprover { allow: true })
    } else {
        Arc::new(ConsoleApprover::new())
    };

    let outcome = Planner::with_actions(&kernel, action_registry)
        .with_approver(approver)
        .with_config(RuntimeConfig {
            auto_approve_from_evolution: auto_approve_evolution,
        })
        .run_goal(&engine, goal, agent_id, &run_id)
        .await;
    match &outcome {
        Ok(res) => {
            println!(
                "✅ Agent 运行完成：run_id={} status={}",
                res.run_id,
                res.status.as_str()
            );
            let events = kernel.list_events(TraceQuery::for_run(&run_id)).await?;
            for e in &events {
                println!(
                    "  [{}] refs={:?} conf={:?} — {}",
                    e.event_type,
                    e.object_refs,
                    e.confidence,
                    e.reasoning.clone().unwrap_or_default()
                );
            }
        }
        Err(PlannerError::EngineUnavailable(m)) => {
            println!("❌ WB2004 认知引擎不可用（Pi 未就绪/崩溃/鉴权失败）：{m}");
        }
        Err(PlannerError::PolicyDenied(m)) => {
            println!("❌ WB2005 Policy 拒绝（需 Human Approval）：{m}");
        }
        Err(PlannerError::Storage(e)) => {
            println!("❌ 存储错误：{e}");
        }
        Err(PlannerError::Action(e)) => {
            println!("❌ 动作执行失败（V0.4 Action Engine）：{e}");
        }
    }
    // Phase 11 11A：RunEnd → Evaluation（真实热路径）。
    // 1) kernel trace 事实桥进 SessionLog（policy.checked / tool.call / approval.*）
    // 2) on_run_end：evaluate（纯函数）→ 落 wb_evaluation → 发射 evaluation.created
    // 3) 成败皆评（失败 run 的评测同样有诊断价值）
    let trace = kernel.list_events(TraceQuery::for_run(&run_id)).await?;
    append_bridged(&mut log, trace_to_session_events(&trace));
    log.append(EventType::SessionCompleted, json!({ "run_id": run_id }));
    // task_id = goal（稳定键）：同目标跨 run 聚合，供诊断环累计证据
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
        &kernel.storage(),
    )
    .await
    {
        Ok(r) => {
            println!(
                "📊 Phase 11 评测（wb_evaluation）：aggregate={:.3} unknowns={:?}",
                r.aggregate,
                r.unknowns()
            );
            println!(
                "   correctness={:?} policy_compliance={:?} reliability={:?}",
                r.correctness.value, r.policy_compliance.value, r.reliability.value
            );
        }
        Err(e) => println!("⚠️ 评测失败（不影响运行结果）：{e}"),
    }
    // Phase 11B 诊断环：失败模式 → Mutation Proposal（等待人工评审，Proposal ≠ Mutation）
    match openaip_os::run_diagnosis(
        &kernel.storage(),
        agent_id,
        "v1",
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
    let policy = policy_for_capabilities(&caps);

    let router = WorkerRouter::new(RouterCriteria {
        min_quality: 0.0,
        max_cost: u32::MAX,
        max_risk: 10,
        explicit: explicit_worker.map(String::from),
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

    let mut orch = WorkerOrchestrator::new(&registry, &policy, router);
    let result = orch.run_plan(&tasks).await;
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
    let kernel = Arc::new(RuntimeKernel::new(storage));
    let engine: Arc<dyn AgentEngine> = Arc::new(PiAgentEngine::new("retention-agent"));
    let registry = load_registry(odl)?;
    let addr: SocketAddr = format!("{host}:{port}").parse()?;
    let listener = TcpListener::bind(addr).await?;
    println!(
        "🚀 OpenAIP-OS Agent Observation 服务已启动：\n  UI  → http://{addr}/\n  API → http://{addr}/api/v1/wb/agent\n  WS  → ws://{addr}/api/v1/wb/agent/{{runId}}/stream\n  认证 → 已强制（V0.7）；先 POST /api/v1/wb/auth/login 取令牌"
    );
    server::serve(
        kernel,
        engine,
        listener,
        static_dir,
        registry,
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

/// 从 ODL 文件构建动作注册表（V0.4）。
fn load_registry(odl: Option<&str>) -> Result<ActionRegistry, Box<dyn std::error::Error>> {
    let path = odl.unwrap_or("examples/crm.odl");
    let src = std::fs::read_to_string(path)?;
    let ir = parse_odl(&src)?;
    Ok(ActionRegistry::from_ir(&ir))
}
