//! V0.2 World Model 测试契约（对应 REQ-012 / REQ-013）
//! TDD: 本文件先于实现落地（Red），由 WB-TASK-009~011 实现转 Green。

use openaip_os::runtime::{RuntimeKernel, TraceKind};
use openaip_os::storage::{LibSQLStorage, ObjectEngine};

/// TC-012 (REQ-012): Relation 建边 + 图查询 + 产生 world 事件(RelationLinked)
#[tokio::test]
async fn tc012_relation_link_and_neighbors() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    storage
        .create("Customer", "001", serde_json::json!({"name": "Apple"}))
        .await
        .unwrap();
    storage
        .create("Account", "002", serde_json::json!({"name": "Acc1"}))
        .await
        .unwrap();

    let kernel = RuntimeKernel::new(storage);
    kernel.link_relation("001", "owns", "002").await.unwrap();

    // 图查询：从 001 出发的邻居含 002（edge=owns）
    let nbrs = kernel.neighbors("001").await.unwrap();
    assert!(
        nbrs.iter()
            .any(|r| r.target == "002" && r.relation == "owns"),
        "邻居应含 002(owns)"
    );

    // 产生 world 事件 RelationLinked，绑定 [src, dst]
    let events = kernel.list_world_events().await.unwrap();
    let linked = events
        .iter()
        .find(|e| e.event_type == "RelationLinked")
        .expect("应产生 RelationLinked 事件");
    assert_eq!(linked.kind, TraceKind::World);
    assert_eq!(
        linked.object_refs,
        vec!["001".to_string(), "002".to_string()]
    );
}

/// TC-012-b (REQ-012): 重复建立相同边被拒（WB2007），不重复落库
#[tokio::test]
async fn tc012b_duplicate_edge_rejected() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    storage
        .create("Customer", "001", serde_json::json!({}))
        .await
        .unwrap();
    storage
        .create("Account", "002", serde_json::json!({}))
        .await
        .unwrap();
    let kernel = RuntimeKernel::new(storage);
    kernel.link_relation("001", "owns", "002").await.unwrap();

    let err = kernel
        .link_relation("001", "owns", "002")
        .await
        .unwrap_err();
    assert!(
        format!("{err}").contains("WB2007"),
        "重复边应返回 WB2007，实际: {err}"
    );

    // 不应重复落库：第二次查询仍只有一条
    assert_eq!(kernel.neighbors("001").await.unwrap().len(), 1);
}

/// TC-013 (REQ-013): 领域 Event（PaymentLate）经 TraceEvent(world) 落库 + 总线发布
#[tokio::test]
async fn tc013_domain_event_emitted() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    storage
        .create("Customer", "001", serde_json::json!({}))
        .await
        .unwrap();
    let kernel = RuntimeKernel::new(storage);

    let mut rx = kernel.event_bus().subscribe();
    kernel
        .emit_domain_event("PaymentLate", &["001"])
        .await
        .unwrap();

    // 落库：world 事件 PaymentLate，绑定 [001]
    let events = kernel.list_world_events().await.unwrap();
    let ev = events
        .iter()
        .find(|e| e.event_type == "PaymentLate")
        .expect("应产生 PaymentLate 事件");
    assert_eq!(ev.kind, TraceKind::World);
    assert_eq!(ev.object_refs, vec!["001".to_string()]);

    // 总线收到同一事件
    let bus_ev = rx.try_recv().expect("总线应收到领域事件");
    assert_eq!(bus_ev.event_type, "PaymentLate");
    assert_eq!(bus_ev.object_refs, vec!["001".to_string()]);
}
