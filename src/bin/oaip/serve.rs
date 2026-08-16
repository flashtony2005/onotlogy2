//! 观察服务命令（M5 拆分）：serve / health。

use openaip_os::abi::odl::parse_odl;
use openaip_os::auth::{IdentityProvider, LocalIdentityProvider};
use openaip_os::server;
use openaip_os::storage::LibSQLStorage;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;

pub async fn cmd_health(db: &str) -> Result<(), Box<dyn std::error::Error>> {
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
pub async fn cmd_serve(
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
    let services = openaip_os::compose::defs::ir_to_services(&odl_ir);
    let effect_defs = openaip_os::compose::defs::ir_to_runtime(&odl_ir);
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
