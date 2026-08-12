//! `oaip` —— OpenAIP-OS 命令行（V0.1：compile / run / object list / event list；V0.2：relation / event emit；V0.3：agent run / serve）

use clap::{Parser, Subcommand};
use openaip_os::action::ActionRegistry;
use openaip_os::agent::{AgentEngine, PiAgentEngine};
use openaip_os::planner::{
    AutoApprover, ConsoleApprover, Planner, PlannerError, PolicyApprover, RuntimeConfig,
    generate_run_id,
};
use openaip_os::runtime::RuntimeKernel;
use openaip_os::server;
use openaip_os::storage::{LibSQLStorage, ObjectEngine, TraceQuery};
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
    /// 启动 Agent 观察服务（HTTP + WebSocket + 三层观察 UI）
    Serve {
        #[arg(long, default_value_t = 8080)]
        port: u16,
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value = "ui/dist")]
        static_dir: String,
        /// 可选 ODL 文件：声明对象自有动作（V0.4 完整 Action Engine）；缺省走 legacy 兜底。
        #[arg(long)]
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
        /// 可选 ODL 文件：声明对象自有动作（V0.4 完整 Action Engine）；缺省走 legacy 兜底。
        #[arg(long)]
        odl: Option<String>,
        /// 自动批准所有需审批动作（跳过交互式 Human Approval）；默认 false → 经 stdin 交互确认。
        #[arg(long, default_value_t = false)]
        auto_approve: bool,
        /// REQ-018 Evolution 第二阶段 AutoApprove：允许基于已验证 LearnedKnowledge 自动放行 require_approval 动作。默认 false → 等同 MVP。
        #[arg(long, default_value_t = false)]
        auto_approve_evolution: bool,
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
            } => {
                cmd_agent_run(
                    &cli.db,
                    &goal,
                    &agent_id,
                    odl.as_deref(),
                    auto_approve,
                    auto_approve_evolution,
                )
                .await?
            }
        },
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
async fn cmd_agent_run(
    db: &str,
    goal: &str,
    agent_id: &str,
    odl: Option<&str>,
    auto_approve: bool,
    auto_approve_evolution: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let storage = LibSQLStorage::open(db).await?;
    let kernel = RuntimeKernel::new(storage);
    let engine = PiAgentEngine::new(agent_id.to_string());
    let registry = load_registry(odl)?;

    if !engine.health().await {
        println!(
            "❌ WB2004 认知引擎不可用：未找到 `pi` 可执行文件。请先安装 @earendil-works/pi-coding-agent 并配置 LLM Provider。"
        );
        return Ok(());
    }

    // Human Approval 闸门：默认 ConsoleApprover（stdin 交互确认）；--auto-approve 直接放行。
    let approver: Arc<dyn PolicyApprover> = if auto_approve {
        Arc::new(AutoApprover { allow: true })
    } else {
        Arc::new(ConsoleApprover::new())
    };

    let run_id = generate_run_id();
    match Planner::with_actions(&kernel, registry)
        .with_approver(approver)
        .with_config(RuntimeConfig {
            auto_approve_from_evolution: auto_approve_evolution,
        })
        .run_goal(&engine, goal, agent_id, &run_id)
        .await
    {
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
    let kernel = Arc::new(RuntimeKernel::new(storage));
    let engine: Arc<dyn AgentEngine> = Arc::new(PiAgentEngine::new("retention-agent"));
    let registry = load_registry(odl)?;
    let addr: SocketAddr = format!("{host}:{port}").parse()?;
    let listener = TcpListener::bind(addr).await?;
    println!(
        "🚀 OpenAIP-OS Agent Observation 服务已启动：\n  UI  → http://{addr}/\n  API → http://{addr}/api/v1/wb/agent\n  WS  → ws://{addr}/api/v1/wb/agent/{{runId}}/stream"
    );
    server::serve(
        kernel,
        engine,
        listener,
        static_dir,
        registry,
        auto_approve,
        auto_approve_evolution,
    )
    .await?;
    Ok(())
}

/// 从 ODL 文件构建动作注册表（V0.4）；缺省返回空注册表（legacy 兜底）。
fn load_registry(odl: Option<&str>) -> Result<ActionRegistry, Box<dyn std::error::Error>> {
    match odl {
        Some(path) => {
            let src = std::fs::read_to_string(path)?;
            let ir = parse_odl(&src)?;
            Ok(ActionRegistry::from_ir(&ir))
        }
        None => Ok(ActionRegistry::empty()),
    }
}
