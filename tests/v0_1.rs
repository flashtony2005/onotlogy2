//! V0.1 全链路测试契约（对应 docs/07_测试文档/test-case.md TC-001~008）
//! 一条用例定义一个能力的边界（Red→Green）。

use openaip_os::codegen;
use openaip_os::ir::{OntologyIR, PropertyDef, ScalarType};
use openaip_os::parse_odl;
use openaip_os::runtime::{RuntimeKernel, TraceEvent, TraceKind};
use openaip_os::storage::{LibSQLStorage, ObjectEngine, ObjectRecord, StorageError};
use serde_json::json;

// ── TC-001 / TC-001-b：ODL Parser（REQ-001） ──────────────────────────────
#[test]
fn tc001_parse_valid() {
    let odl = "object Customer:\n  properties:\n    name: string\n    level: string\n";
    let ir = parse_odl(odl).unwrap();
    let ot = ir.find("Customer").expect("应包含 Customer");
    assert_eq!(ot.properties.len(), 2);
    assert_eq!(
        ot.properties[0],
        PropertyDef {
            name: "name".into(),
            ty: ScalarType::String
        }
    );
    assert_eq!(ot.properties[1].ty, ScalarType::String);
}

#[test]
fn tc001b_parse_invalid() {
    // 未知类型 → 错误码 1002，不 panic
    let e = parse_odl("object Customer:\n  properties:\n    name: unknown\n").unwrap_err();
    assert_eq!(e.code, 1002);

    // 缺 property → 错误码 1001
    let e2 = parse_odl("object Empty:\n  properties:\n").unwrap_err();
    assert_eq!(e2.code, 1001);

    // 属性游离于 properties 块外 → 1005
    let e3 = parse_odl("object X:\n  name: string\n").unwrap_err();
    assert_eq!(e3.code, 1005);
}

// ── TC-002：IR 保真（REQ-002） ───────────────────────────────────────────
#[test]
fn tc002_ir_fidelity() {
    let odl =
        "object Customer:\n  properties:\n    name: string\n    level: string\n    score: int\n";
    let ir = parse_odl(odl).unwrap();
    assert!(codegen::roundtrip_ok(&ir).unwrap());
    let back: OntologyIR = serde_json::from_str(&codegen::to_json(&ir).unwrap()).unwrap();
    assert_eq!(ir, back);
}

// ── TC-003：Codegen（REQ-003） ──────────────────────────────────────────
#[test]
fn tc003_codegen() {
    let odl = "object Customer:\n  properties:\n    name: string\n    level: string\n";
    let ir = parse_odl(odl).unwrap();
    let out = codegen::codegen(&ir).unwrap();
    assert!(out.rust.contains("pub struct Customer"));
    assert!(out.rust.contains("pub name: String"));
    assert!(out.rust.contains("pub level: String"));
    assert!(out.rust.contains("pub const CUSTOMER_DEF"));
    assert!(out.rust.contains("ObjectDef"));
    assert!(out.schema_sql.contains("wb_ontology_object"));
    // ontology.json 与 IR 一致
    let back: OntologyIR = serde_json::from_str(&out.ontology_json).unwrap();
    assert_eq!(ir, back);
}

// ── TC-004：Object CRUD（REQ-004） ──────────────────────────────────────
#[tokio::test]
async fn tc004_object_crud() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let r = s
        .create("Customer", "001", json!({"name": "Apple", "level": "VIP"}))
        .await
        .unwrap();
    assert_eq!(r.version, 1);

    let got = s.get("001").await.unwrap().unwrap();
    assert_eq!(got.data, json!({"name": "Apple", "level": "VIP"}));

    let u = s
        .update("001", json!({"name": "Apple", "level": "GOLD"}))
        .await
        .unwrap();
    assert_eq!(u.version, 2, "update 必须使 version 自增");

    s.delete("001").await.unwrap();
    assert!(s.get("001").await.unwrap().is_none(), "删除后不可见");
}

// ── TC-005：LibSQLStorage 读写 Turso（REQ-005） ─────────────────────────
#[tokio::test]
async fn tc005_turso_roundtrip() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    s.create("Customer", "001", json!({"name": "Apple", "level": "VIP"}))
        .await
        .unwrap();
    s.create("Customer", "002", json!({"name": "Tesla", "level": "GOLD"}))
        .await
        .unwrap();

    let q = s.query("Customer").await.unwrap();
    assert_eq!(q.len(), 2, "落库数据应可被 query 取回");
    assert_eq!(q[0].id, "001");
    assert_eq!(q[1].data, json!({"name": "Tesla", "level": "GOLD"}));
}

// ── TC-006：CLI 渲染（REQ-006） ─────────────────────────────────────────
#[test]
fn tc006_render_list() {
    let recs = vec![
        ObjectRecord {
            id: "001".into(),
            r#type: "Customer".into(),
            data: json!({"name": "Apple", "level": "VIP"}),
            version: 1,
        },
        ObjectRecord {
            id: "002".into(),
            r#type: "Customer".into(),
            data: json!({"name": "Tesla", "level": "GOLD"}),
            version: 1,
        },
    ];
    let out = LibSQLStorage::render_object_list(&recs);
    assert_eq!(out, "001 Apple VIP\n002 Tesla GOLD");
}

// ── TC-007：TraceEvent(world)（REQ-007） ────────────────────────────────
#[tokio::test]
async fn tc007_world_trace() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let k = RuntimeKernel::new(s);
    k.create_object("Customer", "001", json!({"name": "Apple"}))
        .await
        .unwrap();
    k.update_object("001", json!({"name": "Apple", "level": "VIP"}))
        .await
        .unwrap();
    k.delete_object("001").await.unwrap();

    let events = k.list_world_events().await.unwrap();
    assert_eq!(
        events.len(),
        3,
        "create/update/delete 各产生一条 world 事件"
    );
    for e in &events {
        assert_eq!(e.kind, TraceKind::World);
        assert_eq!(
            e.object_refs,
            vec!["001".to_string()],
            "必须绑定 object_refs"
        );
    }
    assert_eq!(events[0].event_type, "ObjectCreated");
    assert_eq!(events[2].event_type, "ObjectDeleted");
}

#[tokio::test]
async fn tc007b_missing_refs_blocked() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let ev = TraceEvent {
        id: "x".into(),
        kind: TraceKind::World,
        event_type: "ObjectCreated".into(),
        object_refs: vec![], // 缺 object_refs
        run_id: None,
        agent_id: None,
        reasoning: None,
        input: None,
        output: None,
        confidence: None,
        timestamp: "0".into(),
    };
    let err = s.append_trace(&ev).await.unwrap_err();
    match err {
        StorageError::Constraint { code, .. } => assert_eq!(code, "WB2006"),
        _ => panic!("缺 object_refs 应返回 WB2006"),
    }
}

// ── TC-008：Event Bus 分发（REQ-008） ───────────────────────────────────
#[tokio::test]
async fn tc008_bus_publish_consistent() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let k = RuntimeKernel::new(s);
    let mut rx = k.event_bus().subscribe();

    k.create_object("Customer", "001", json!({"name": "Apple"}))
        .await
        .unwrap();

    // 总线收到的事件，与落库事件一致
    let published = rx.recv().await.unwrap();
    let stored = k.list_world_events().await.unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(published, stored[0]);
    assert_eq!(published.event_type, "ObjectCreated");
}

// ── TC-005-b：旧 schema 检测（WB2101，root cause 防护） ─────────────────────
#[tokio::test]
async fn tc005b_open_rejects_legacy_schema() {
    // 真实文件旧 schema（任意外部工具创建的旧列：object_type/from_id/to_id/edge_type，无 wb_meta）。
    // open 必须返 WB2101，而非下游 cryptic 的 no such column。
    let path = std::env::temp_dir().join(format!("oaip_legacy_{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);

    // 用 libsql 造一个“旧库”，模拟外部创建的旧 schema 文件（含 WAL 刷盘）
    {
        let db = libsql::Builder::new_local(&path).build().await.unwrap();
        let conn = db.connect().unwrap();
        conn.execute_batch(
            "CREATE TABLE wb_ontology_object (id TEXT PRIMARY KEY, object_type TEXT, data TEXT);\
             CREATE TABLE wb_relation_edge (id TEXT PRIMARY KEY, edge_type TEXT, from_id TEXT, to_id TEXT);",
        )
        .await
        .unwrap();
        let _ = conn
            .query("PRAGMA wal_checkpoint(TRUNCATE)", libsql::params![])
            .await;
    } // db/conn drop，旧库落盘

    let res = LibSQLStorage::open(path.to_str().unwrap()).await;
    match res {
        Ok(_) => panic!("旧 schema 应返回 WB2101，但实际打开成功"),
        Err(StorageError::Constraint { code, .. }) => assert_eq!(code, "WB2101"),
        Err(other) => panic!("旧 schema 应返回 WB2101，实际: {other:?}"),
    }
    let _ = std::fs::remove_file(&path);
}
