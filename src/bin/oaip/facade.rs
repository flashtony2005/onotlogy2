//! Facade CLI Transport（M5 风格拆分）—— `oaip world query` / `oaip audit query`
//! / `oaip fabric submit`：**只做参数翻译，不持有语义**——全部调用
//! `OpenAipFacade`（契约 facade-v1.md：单一语义源，Transport 无第二套语义）。
//!
//! 执行约定：Facade 是同步 API（内部 block_on storage）→ 调用点包
//! `block_in_place`（multi_thread runtime；与 StorageSink 同约定）。

use std::sync::Arc;

use openaip_os::fabric::adapter::AgentAdapter;
use openaip_os::fabric::adapters::{codex_adapter, dsh_adapter, shell_adapter};
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
    for a in [codex_adapter(None), dsh_adapter(None), shell_adapter(None)] {
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
    project: Option<&str>,
    object_ref: Option<&str>,
    predicate: Option<&str>,
    limit: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let handle = tokio::runtime::Handle::current();
    let facade = cli_facade(db, handle)?;
    let view = facade.world_query(WorldQuery {
        project_id: project.map(String::from),
        object_ref: object_ref.map(String::from),
        predicate: predicate.map(String::from),
        limit,
    })?;
    match project {
        Some(p) => println!(
            "🌍 World View（project={p}, at={}, total={}）：",
            view.at, view.total
        ),
        None => println!("🌍 World View（at={}, total={}）：", view.at, view.total),
    }
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
        project_id: None,
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
        project_id: None,
    })?;
    println!("🚀 Fabric 提交：");
    println!(
        "   sender={} → receiver={} correlation={:?}",
        sub.result.sender, sub.result.receiver, sub.result.correlation_id
    );
    println!(
        "   status={} payload={}",
        sub.result.payload["status"], sub.result.payload
    );
    // T2b-2 异步契约：running = 子进程已在后台执行（spawn_blocking）。CLI 需阻塞到终局，
    // 否则进程退出会杀掉后台任务。轮询审计域 fabric.* 的 correlation_id 对应事件。
    if sub.result.payload["status"] == "running" {
        let cid = sub.result.correlation_id.clone().unwrap_or_default();
        // 已在 block_in_place 内：audit_query 是同步 API（内部 block_on storage），直接调用。
        let mut final_status: Option<String> = None;
        for _ in 0..300 {
            let view = match facade.audit_query(AuditQuery {
                event_type: Some("fabric.".into()),
                ..Default::default()
            }) {
                Ok(v) => v,
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    continue;
                }
            };
            for e in &view.events {
                if e.payload.get("correlation_id").and_then(|c| c.as_str()) == Some(cid.as_str())
                    && let Some(s) = e.payload.get("to").and_then(|s| s.as_str())
                {
                    final_status = Some(s.to_string());
                    break;
                }
            }
            if final_status.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        match final_status {
            Some(s) => println!("   🏁 后台执行终局: status={}", s),
            None => println!("   ⏳ 后台执行超时未终局（correlation={}）", cid),
        }
    }
    Ok(())
}

pub fn cmd_project_list(db: &str) -> Result<(), Box<dyn std::error::Error>> {
    let handle = tokio::runtime::Handle::current();
    let facade = cli_facade(db, handle)?;
    let projects = facade.project_list()?;
    println!("📁 项目（{}）：", projects.len());
    for p in &projects {
        println!(
            "   {} | {} | {} | ontology={} 字符",
            p.id,
            p.name,
            p.status,
            p.ontology.len()
        );
    }
    Ok(())
}

/// seed OpenAIP-OS 工程本体：工程事实（模块/文件/测试）→ 项目世界事件。
pub fn cmd_project_seed(db: &str) -> Result<(), Box<dyn std::error::Error>> {
    let handle = tokio::runtime::Handle::current();
    let storage = Arc::new(handle.block_on(LibSQLStorage::open(db))?);
    let n = handle.block_on(openaip_os::project_seed::seed_openaip_os(&storage, "."))?;
    println!(
        "🌱 OpenAIP-OS 工程本体 seed 完成：{} 条世界事件 → project=openaip-os",
        n
    );
    let facade = cli_facade(db, handle)?;
    let view = facade.world_query(openaip_os::facade::WorldQuery {
        project_id: Some("openaip-os".into()),
        limit: 1000,
        ..Default::default()
    })?;
    println!("   项目世界：{} 条关系（模块/文件/测试结构）", view.total);
    Ok(())
}
