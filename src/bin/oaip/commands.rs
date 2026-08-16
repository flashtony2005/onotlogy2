//! 世界与编译命令（M5 拆分）：compile / run / object / relation / event / db。

use openaip_os::abi::codegen;
use openaip_os::abi::odl::parse_odl;
use openaip_os::storage::{LibSQLStorage, ObjectEngine, RelationEngine};
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

pub async fn cmd_run(db: &str) -> Result<(), Box<dyn std::error::Error>> {
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
