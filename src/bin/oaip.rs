//! `oaip` —— OpenAIP-OS 命令行（V0.1：compile / run / object list / event list；V0.2：relation / event emit）

use clap::{Parser, Subcommand};
use openaip_os::runtime::RuntimeKernel;
use openaip_os::storage::{LibSQLStorage, ObjectEngine};
use openaip_os::{codegen, parse_odl};
use serde_json::json;

#[derive(Parser)]
#[command(name = "oaip", version, about = "OpenAIP-OS 可执行本体平台 CLI (V0.2)")]
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
    /// 数据库维护（schema 迁移 / 清理演示数据）
    Db {
        #[command(subcommand)]
        cmd: DbCmd,
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
