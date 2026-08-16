//! Facade CLI Transport（M5 风格拆分）—— `oaip world query` / `oaip audit query`
//! / `oaip fabric submit`：**只做参数翻译，不持有语义**——全部调用
//! `OpenAipFacade`（契约 facade-v1.md：单一语义源，Transport 无第二套语义）。
//!
//! 执行约定：Facade 是同步 API（内部 block_on storage）→ 调用点包
//! `block_in_place`（multi_thread runtime；与 StorageSink 同约定）。

use std::sync::Arc;

use openaip_os::fabric::adapter::AgentAdapter;
use openaip_os::fabric::adapters::{codex_adapter, dsh_adapter};
use openaip_os::fabric::router::FabricRouter;
use openaip_os::facade::{AuditQuery, FabricRequest, Facade, OpenAipFacade, WorldQuery};
use openaip_os::storage::LibSQLStorage;

/// CLI Facade 装配：意图映射表固定；适配器**动态发现**——connect 成功的
/// Agent 把 discover 的能力注册进 Router（Codex/DSH 是 Capability Provider）。
pub fn cli_facade(
    db: &str,
    handle: tokio::runtime::Handle,
) -> Result<Facade, Box<dyn std::error::Error>> {
    let storage = Arc::new(handle.block_on(LibSQLStorage::open(db))?);
    let mut router = FabricRouter::new();
    router.register_intent("modify_code", "code.write");
    router.register_intent("run_shell", "shell.execute");
    router.register_intent("run_tests", "code.test");
    router.mark_high_risk("shell.execute");

    let mut adapters: Vec<Arc<dyn AgentAdapter>> = Vec::new();
    for a in [codex_adapter(None), dsh_adapter(None)] {
        if a.connect().is_ok() {
            let id = a.id().to_string();
            for c in a.discover() {
                router.register_agent(c.name, id.clone(), 1);
            }
            adapters.push(Arc::new(a));
        } else {
            println!("⚠️ 适配器离线（命令不可用）：{}", a.id());
        }
    }
    let facade = Facade::new(storage, handle, router, adapters, &[])?;
    Ok(facade)
}

pub fn cmd_world_query(
    db: &str,
    object_ref: Option<&str>,
    predicate: Option<&str>,
    limit: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let handle = tokio::runtime::Handle::current();
    let facade = cli_facade(db, handle)?;
    let view = facade.world_query(WorldQuery {
        object_ref: object_ref.map(String::from),
        predicate: predicate.map(String::from),
        limit,
    })?;
    println!("🌍 World View（at={}, total={}）：", view.at, view.total);
    for r in &view.relations {
        let until = r
            .valid_until
            .map(|u| u.to_string())
            .unwrap_or_else(|| "∞".into());
        println!(
            "   {} --{}--> {}  ({} → {})",
            r.source, r.predicate, r.target, r.valid_from, until
        );
    }
    Ok(())
}

pub fn cmd_audit_query(
    db: &str,
    run_id: Option<&str>,
    event_type: Option<&str>,
    limit: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let handle = tokio::runtime::Handle::current();
    let facade = cli_facade(db, handle)?;
    let view = facade.audit_query(AuditQuery {
        run_id: run_id.map(String::from),
        agent_id: None,
        event_type: event_type.map(String::from),
        limit,
    })?;
    println!("📋 Audit View（total={}）：", view.total);
    for e in &view.events {
        println!(
            "   [{}] {} refs={:?} {}",
            e.timestamp, e.event_type, e.refs, e.payload
        );
    }
    Ok(())
}

pub fn cmd_fabric_submit(
    db: &str,
    session: &str,
    sender: &str,
    intent: &str,
    payload: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let handle = tokio::runtime::Handle::current();
    let facade = cli_facade(db, handle)?;
    let payload: serde_json::Value = serde_json::from_str(payload)?;
    let sub = facade.fabric_submit(FabricRequest {
        session_id: session.to_string(),
        sender: sender.to_string(),
        intent: intent.to_string(),
        payload,
    })?;
    println!("🚀 Fabric 提交完成：");
    println!(
        "   sender={} → receiver={} correlation={:?}",
        sub.result.sender, sub.result.receiver, sub.result.correlation_id
    );
    println!(
        "   status={} payload={}",
        sub.result.payload["status"], sub.result.payload
    );
    Ok(())
}
