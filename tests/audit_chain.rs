//! Phase 2.2：审计哈希链、世界事务与稳定导出契约。

use openaip_os::storage::{LibSQLStorage, TraceQuery, WorldCommand};
use openaip_os::trace::{TraceEvent, TraceKind};
use serde_json::{Value, json};

fn event(id: &str, event_type: &str) -> TraceEvent {
    TraceEvent {
        id: id.into(),
        kind: TraceKind::Cognitive,
        event_type: event_type.into(),
        object_refs: vec!["audit-test".into()],
        project_id: None,
        run_id: Some("run-audit-test".into()),
        agent_id: Some("test".into()),
        reasoning: Some("deterministic test".into()),
        input: Some(json!({ "id": id })),
        output: None,
        confidence: Some(1.0),
        timestamp: id.into(),
    }
}

#[tokio::test]
async fn audit_chain_is_append_only_and_export_is_stable() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    storage
        .append_trace(&event("event-1", "TestStarted"))
        .await
        .unwrap();
    storage
        .append_trace(&event("event-2", "TestFinished"))
        .await
        .unwrap();

    storage.validate_audit_chain().await.unwrap();
    let chain = storage.list_audit_chain().await.unwrap();
    assert_eq!(chain.len(), 2);
    assert_eq!(chain[0].sequence, 1);
    assert_eq!(chain[1].sequence, 2);
    assert_eq!(chain[1].previous_hash, chain[0].hash);

    let export: Value = serde_json::from_str(&storage.audit_export_json().await.unwrap()).unwrap();
    assert_eq!(export["format"], "oaip.audit.export.v1");
    assert_eq!(export["hash_algorithm"], "SHA-256");
    assert_eq!(export["event_count"], 2);
    assert_eq!(export["head_hash"], chain[1].hash);
    assert_eq!(export["events"][0]["sequence"], 1);
    assert_eq!(export["events"][1]["event"]["id"], "event-2");
}

#[tokio::test]
async fn world_transactions_append_to_the_same_audit_chain() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    storage
        .apply_world(WorldCommand::CreateObject {
            object_type: "Customer".into(),
            id: "customer-1".into(),
            data: json!({ "name": "Ada" }),
            project_id: None,
        })
        .await
        .unwrap();
    storage
        .append_trace(&event("event-3", "AfterWorldCommit"))
        .await
        .unwrap();

    let events = storage
        .list_events(TraceQuery {
            limit: 100,
            ..Default::default()
        })
        .await
        .unwrap();
    let chain = storage.list_audit_chain().await.unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(chain.len(), events.len());
    storage.validate_audit_chain().await.unwrap();
}

#[tokio::test]
async fn auth_key_rotation_rolls_back_sessions_when_audit_append_fails() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    storage
        .save_session("session-hash", "user-1", "9999999999")
        .await
        .unwrap();

    let mut invalid = event("rotation-invalid", "auth.key_rotated");
    invalid.object_refs.clear();
    let err = storage
        .rotate_auth_key(&invalid)
        .await
        .expect_err("非法审计事件必须阻止轮换");
    assert!(err.to_string().contains("WB2006"));
    assert!(
        storage.get_session("session-hash").await.unwrap().is_some(),
        "审计写入失败时旧会话必须保持有效"
    );
    assert!(storage.list_audit_chain().await.unwrap().is_empty());
}

#[tokio::test]
async fn projected_audit_export_contains_export_event_without_mutating_chain() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    let exported = event("export-1", "audit.exported");
    let payload: Value = serde_json::from_str(
        &storage
            .audit_export_json_with_event(Some(&exported))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(payload["event_count"], 1);
    assert_eq!(payload["events"][0]["event"]["id"], "export-1");
    assert_eq!(
        payload["events"][0]["event"]["event_type"],
        "audit.exported"
    );
    assert!(storage.list_audit_chain().await.unwrap().is_empty());

    storage.append_trace(&exported).await.unwrap();
    let committed: Value =
        serde_json::from_str(&storage.audit_export_json().await.unwrap()).unwrap();
    assert_eq!(payload, committed);
}
