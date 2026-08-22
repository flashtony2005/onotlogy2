//! 世界与编译命令（M5 拆分）：compile / run / object / relation / event / db。

use openaip_os::abi::codegen;
use openaip_os::abi::odl::parse_odl;
use openaip_os::storage::{LibSQLStorage, ObjectEngine, RelationEngine};
use openaip_os::trace::{TraceEvent, TraceKind, new_trace_id, now_ts};
use serde_json::json;

pub async fn cmd_compile(file: &str) -> Result<(), Box<dyn std::error::Error>> {
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

pub async fn cmd_backup(db: &str, output: &str) -> Result<(), Box<dyn std::error::Error>> {
    ensure_local_database_path(db)?;
    let storage = LibSQLStorage::open(db).await?;
    storage.backup_to(output).await?;
    println!("✅ 备份完成：{db} → {output}");
    Ok(())
}

pub async fn cmd_restore(db: &str, input: &str) -> Result<(), Box<dyn std::error::Error>> {
    ensure_local_database_path(db)?;
    LibSQLStorage::restore_from(input, db).await?;
    // 安装后再次走正常 open，确保目标库的 schema 与当前代码一致。
    LibSQLStorage::open(db).await?;
    println!("✅ 恢复完成：{input} → {db}");
    Ok(())
}

pub async fn cmd_audit_export(db: &str, output: &str) -> Result<(), Box<dyn std::error::Error>> {
    ensure_local_database_path(db)?;
    let storage = LibSQLStorage::open(db).await?;
    // 先验链并预构造包含 audit.exported 的内容；文件写入失败时不推进审计链。
    storage.validate_audit_chain().await?;
    let event = TraceEvent {
        id: new_trace_id(),
        kind: TraceKind::Cognitive,
        event_type: "audit.exported".into(),
        object_refs: vec!["audit".into()],
        project_id: None,
        run_id: None,
        agent_id: Some("oaip-cli".into()),
        reasoning: None,
        input: Some(json!({ "format": "oaip.audit.export.v1" })),
        output: None,
        confidence: None,
        timestamp: now_ts(),
    };
    let payload = storage.audit_export_json_with_event(Some(&event)).await?;
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    if let Err(error) = file
        .write_all(payload.as_bytes())
        .and_then(|_| file.write_all(b"\n"))
    {
        drop(file);
        let _ = std::fs::remove_file(output);
        return Err(error.into());
    }
    drop(file);
    if let Err(error) = storage.append_trace(&event).await {
        let _ = std::fs::remove_file(output);
        return Err(error.into());
    }
    println!("✅ 审计导出完成：{db} → {output}");
    Ok(())
}

fn ensure_local_database_path(db: &str) -> Result<(), Box<dyn std::error::Error>> {
    if LibSQLStorage::is_remote_url(db) || db == ":memory:" {
        return Err(format!("backup/restore 只支持本地数据库文件，当前路径: {db}").into());
    }
    Ok(())
}

pub async fn cmd_run(db: &str) -> Result<(), Box<dyn std::error::Error>> {
    use openaip_os::storage::WorldCommand;
    let storage = LibSQLStorage::open(db).await?;
    // 演示数据（P5.3）：WorldCommand 事务化——对象写 + ObjectCreated 事件同事务落库，
    // 消灭「无事件的世界变更」（P5/P7 契约）。
    storage
        .apply_world_many(&[
            WorldCommand::CreateObject {
                object_type: "Customer".into(),
                id: "001".into(),
                data: json!({ "name": "Apple", "level": "VIP" }),
                project_id: None,
            },
            WorldCommand::CreateObject {
                object_type: "Customer".into(),
                id: "002".into(),
                data: json!({ "name": "Tesla", "level": "GOLD" }),
                project_id: None,
            },
        ])
        .await?;
    let recs = storage.query("Customer").await?;
    println!("{}", LibSQLStorage::render_object_list(&recs));
    Ok(())
}

pub async fn cmd_object_list(
    db: &str,
    object_type: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let storage = LibSQLStorage::open(db).await?;
    let recs = storage.query(object_type).await?;
    if recs.is_empty() {
        println!("(无 {object_type} 对象，先运行 `oaip run`)");
    } else {
        println!("{}", LibSQLStorage::render_object_list(&recs));
    }
    Ok(())
}

pub async fn cmd_event_list(db: &str) -> Result<(), Box<dyn std::error::Error>> {
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

pub async fn cmd_relation_link(
    db: &str,
    source: &str,
    edge: &str,
    target: &str,
    project: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let storage = LibSQLStorage::open(db).await?;
    match storage
        .apply_world(openaip_os::storage::WorldCommand::LinkRelation {
            source: source.to_string(),
            edge: edge.to_string(),
            target: target.to_string(),
            project_id: project.map(String::from),
        })
        .await
    {
        Ok(()) => {
            println!("✅ 已建立关系: {source} -{edge}-> {target}（RelationLinked 事件已落库）")
        }
        Err(e) => println!("❌ 建立关系失败：{e}"),
    }
    Ok(())
}

pub async fn cmd_relation_list(
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

pub async fn cmd_event_emit(
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

pub async fn cmd_db_reset(db: &str) -> Result<(), Box<dyn std::error::Error>> {
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
