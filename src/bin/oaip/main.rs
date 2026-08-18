//! `oaip` —— OpenAIP-OS 命令行（M5 拆分：子命令按域归模块）。
//!
//! 模块：
//! - `commands`：compile / run / object / relation / event / db
//! - `agent`：agent run（V2 闭环 + via_workers 桥）
//! - `worker`：worker run + 预置 Worker 环境
//! - `sim`：replay / simulate
//! - `serve`：serve / health
//! - `user`：用户与账号管理

mod agent;
mod commands;
mod facade;
mod project_cmd;
mod serve;
mod sim;
mod user;
mod worker;

use clap::{Parser, Subcommand};

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
    /// 编译 ODL → gen/（Rust struct + schema.sql + ontology.json）
    Compile { file: String },
    /// 写入演示数据（Customer 001/002）
    Run,
    /// 对象操作
    Object {
        #[command(subcommand)]
        cmd: ObjectCmd,
    },
    /// Agent 闭环（V2：PiEngine + AgentLoop）
    Agent {
        #[command(subcommand)]
        cmd: AgentCmd,
    },
    /// Worker ABI：Thinker 规划 → Worker 执行（OpenAIP-OS 不亲自干活）
    Worker {
        #[command(subcommand)]
        cmd: WorkerCmd,
    },
    /// 关系边操作
    Relation {
        #[command(subcommand)]
        cmd: RelationCmd,
    },
    /// 事件操作
    Event {
        #[command(subcommand)]
        cmd: EventCmd,
    },
    /// Public Semantic Facade（world / audit / fabric 三语义域；单一语义源）
    Facade {
        #[command(subcommand)]
        cmd: FacadeCmd,
    },
    /// Loop Health 六指标
    Health,
    /// S1 决策回放（Learning Counterfactual）
    Replay,
    /// S2/S3 模拟（Projection / Counterfactual，假设轨永不写 EventLog）
    Simulate {
        #[arg(long)]
        a: String,
        #[arg(long)]
        b: String,
        /// 动作 A 假设派给的 Worker id
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
        /// ODL 文件：声明对象自有动作；缺省用 examples/crm.odl。
        #[arg(long, default_value = "examples/crm.odl")]
        odl: Option<String>,
        /// 自动批准所有需审批动作；默认 false → require_approval 动作挂起。
        #[arg(long, default_value_t = false)]
        auto_approve: bool,
        /// REQ-018 Evolution 第二阶段 AutoApprove 开关。
        #[arg(long, default_value_t = false)]
        auto_approve_evolution: bool,
    },
    /// 数据库维护（schema 迁移 / 重建）
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
enum ObjectCmd {
    List { object_type: String },
}

#[derive(Subcommand)]
enum AgentCmd {
    /// 触发 Agent 运行（Pi 作 Cognitive Engine）：oaip agent run "reduce churn"
    Run {
        /// 自然语言目标
        goal: String,
        /// Agent ID
        #[arg(long, default_value = "retention-agent")]
        agent_id: String,
        /// ODL 文件
        #[arg(long, default_value = "examples/crm.odl")]
        odl: Option<String>,
        /// 自动批准所有需审批动作
        #[arg(long, default_value_t = false)]
        auto_approve: bool,
        /// REQ-018 AutoApprove 开关
        #[arg(long, default_value_t = false)]
        auto_approve_evolution: bool,
        /// V2 桥：Thinker 决策 → Worker 执行
        #[arg(long, default_value_t = false)]
        via_workers: bool,
        /// 执行所用 Skill 版本（写入 wb_evaluation.skill_version）
        #[arg(long)]
        skill: Option<String>,
    },
}

#[derive(Subcommand)]
enum WorkerCmd {
    /// 运行 Worker 计划（能力市场 + 路由 + 策略 + 执行 + 评测）
    Run {
        #[arg(long)]
        capabilities: String,
        #[arg(long)]
        input: Option<String>,
        #[arg(long)]
        task: Option<String>,
        #[arg(long)]
        worker: Option<String>,
        #[arg(long, default_value_t = false)]
        evaluate: bool,
    },
}

#[derive(Subcommand)]
enum RelationCmd {
    Link {
        source: String,
        edge: String,
        target: String,
        #[arg(long)]
        project: Option<String>,
    },
    List {
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        edge: Option<String>,
    },
}

#[derive(Subcommand)]
enum EventCmd {
    List,
    Emit {
        event_type: String,
        refs: Vec<String>,
    },
}

/// Facade 三语义域（Transport 只做参数翻译；语义在 OpenAipFacade）。
/// 动词层与契约一致：`oaip facade world query` / `audit query` / `fabric submit`。
#[derive(Subcommand)]
enum FacadeCmd {
    /// world 语义域
    World {
        #[command(subcommand)]
        cmd: WorldCmd,
    },
    /// audit 语义域
    Audit {
        #[command(subcommand)]
        cmd: AuditCmd,
    },
    /// fabric 语义域
    Fabric {
        #[command(subcommand)]
        cmd: FabricCmd,
    },
    /// project 语义域（Phase 8：项目 = 世界顶层组织单元）
    Project {
        #[command(subcommand)]
        cmd: ProjectCmd,
    },
}

#[derive(Subcommand)]
enum ProjectCmd {
    /// 列出项目
    List,
    /// seed OpenAIP-OS 工程本体（模块/文件/测试 → 项目世界）
    Seed,
    /// 运行 make check → CheckRun 状态入项目世界（工程健康度时间线）
    Check,
    /// 最近 git 提交 → Commit 对象入项目世界
    Commit,
    /// 给项目世界加对象（--type/--id/--project/--input [--edge]）
    Event {
        #[arg(long)]
        object_type: String,
        #[arg(long)]
        id: String,
        #[arg(long, default_value = "openaip-os")]
        project: String,
        #[arg(long)]
        input: String,
        #[arg(long)]
        edge: Option<String>,
    },
    /// 依赖图 seed：crates 依赖 → depends_on 关系入项目世界
    Deps,
}

#[derive(Subcommand)]
enum WorldCmd {
    /// world.query：世界视图（Event Log → Projection → Kernel 只读）
    Query {
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        object_ref: Option<String>,
        #[arg(long)]
        predicate: Option<String>,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
}

#[derive(Subcommand)]
enum AuditCmd {
    /// audit.query：审计语义域（approval.* / fabric.* / evolution.gate_approved / policy.denied）
    Query {
        #[arg(long)]
        run_id: Option<String>,
        #[arg(long)]
        event_type: Option<String>,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
}

#[derive(Subcommand)]
enum FabricCmd {
    /// fabric.submit：Router → Adapter → 事件 → 世界
    Submit {
        #[arg(long)]
        session: String,
        #[arg(long, default_value = "dsh")]
        sender: String,
        #[arg(long)]
        intent: String,
        /// JSON 载荷（args/task/object 等）
        #[arg(long)]
        payload: String,
        /// 轮询后台终局事件的最大等待秒数（默认 30）；0 = 提交后立即返回不等待。
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
}

#[derive(Subcommand)]
enum DbCmd {
    /// 删除库文件并由 canonical_schema 重建
    Reset,
}

#[derive(Subcommand)]
enum UserCmd {
    List,
    CreateAdmin {
        username: String,
        password: String,
    },
    Create {
        username: String,
        password: String,
        role: String,
        display_name: String,
    },
    Delete {
        username: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Compile { file } => commands::cmd_compile(&file).await?,
        Cmd::Run => commands::cmd_run(&cli.db).await?,
        Cmd::Object { cmd } => match cmd {
            ObjectCmd::List { object_type } => {
                commands::cmd_object_list(&cli.db, &object_type).await?
            }
        },
        Cmd::Relation { cmd } => match cmd {
            RelationCmd::Link {
                source,
                edge,
                target,
                project,
            } => {
                commands::cmd_relation_link(&cli.db, &source, &edge, &target, project.as_deref())
                    .await?
            }
            RelationCmd::List { from, edge } => {
                commands::cmd_relation_list(&cli.db, from, edge).await?
            }
        },
        Cmd::Event { cmd } => match cmd {
            EventCmd::List => commands::cmd_event_list(&cli.db).await?,
            EventCmd::Emit { event_type, refs } => {
                commands::cmd_event_emit(&cli.db, &event_type, &refs).await?
            }
        },
        Cmd::Facade { cmd } => match cmd {
            // Facade 是同步 API（内部 block_on storage）→ 阻塞上下文约定
            FacadeCmd::World { cmd } => match cmd {
                WorldCmd::Query {
                    project,
                    object_ref,
                    predicate,
                    limit,
                } => tokio::task::block_in_place(|| {
                    facade::cmd_world_query(
                        &cli.db,
                        project.as_deref(),
                        object_ref.as_deref(),
                        predicate.as_deref(),
                        limit,
                    )
                })?,
            },
            FacadeCmd::Audit { cmd } => match cmd {
                AuditCmd::Query {
                    run_id,
                    event_type,
                    limit,
                } => tokio::task::block_in_place(|| {
                    facade::cmd_audit_query(
                        &cli.db,
                        run_id.as_deref(),
                        event_type.as_deref(),
                        limit,
                    )
                })?,
            },
            FacadeCmd::Fabric { cmd } => match cmd {
                FabricCmd::Submit {
                    session,
                    sender,
                    intent,
                    payload,
                    timeout,
                } => tokio::task::block_in_place(|| {
                    facade::cmd_fabric_submit(
                        &cli.db, &session, &sender, &intent, &payload, timeout,
                    )
                })?,
            },
            FacadeCmd::Project { cmd } => match cmd {
                ProjectCmd::List => {
                    tokio::task::block_in_place(|| facade::cmd_project_list(&cli.db))?
                }
                ProjectCmd::Seed => {
                    tokio::task::block_in_place(|| facade::cmd_project_seed(&cli.db))?
                }
                ProjectCmd::Check => {
                    tokio::task::block_in_place(|| project_cmd::cmd_project_check(&cli.db))?
                }
                ProjectCmd::Commit => {
                    tokio::task::block_in_place(|| project_cmd::cmd_project_commit(&cli.db))?
                }
                ProjectCmd::Event {
                    object_type,
                    id,
                    project,
                    input,
                    edge,
                } => tokio::task::block_in_place(|| {
                    project_cmd::cmd_project_event(
                        &cli.db,
                        &object_type,
                        &id,
                        &project,
                        &input,
                        edge.as_deref(),
                    )
                })?,
                ProjectCmd::Deps => {
                    tokio::task::block_in_place(|| project_cmd::cmd_project_deps(&cli.db))?
                }
            },
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
                agent::cmd_agent_run(
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
                worker::cmd_worker_run(
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
        Cmd::Health => serve::cmd_health(&cli.db).await?,
        Cmd::Replay => sim::cmd_replay(&cli.db).await?,
        Cmd::Simulate {
            a,
            b,
            worker_a,
            worker_b,
        } => sim::cmd_simulate(&cli.db, &a, &b, worker_a.as_deref(), worker_b.as_deref()).await?,
        Cmd::Serve {
            port,
            host,
            static_dir,
            odl,
            auto_approve,
            auto_approve_evolution,
        } => {
            serve::cmd_serve(
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
            DbCmd::Reset => commands::cmd_db_reset(&cli.db).await?,
        },
        Cmd::User { cmd } => match cmd {
            UserCmd::List => user::cmd_user_list(&cli.db).await?,
            UserCmd::CreateAdmin { username, password } => {
                user::cmd_user_create_admin(&cli.db, &username, &password).await?
            }
            UserCmd::Create {
                username,
                password,
                role,
                display_name,
            } => user::cmd_user_create(&cli.db, &username, &password, &role, &display_name).await?,
            UserCmd::Delete { username } => user::cmd_user_delete(&cli.db, &username).await?,
        },
    }
    Ok(())
}
