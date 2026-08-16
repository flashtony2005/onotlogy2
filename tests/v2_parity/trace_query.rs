//! M1b · G9 契约测试：TraceQuery 过滤（v0_3 tc013 的 V2 等价）。
//!
//! storage 事件查询层在 M1d 后存活（UI 时间线 REST / WS 重连补齐依赖）；
//! 本契约钉住：按 run / event_type / after_id 游标过滤。

use openaip_os::storage::{LibSQLStorage, TraceQuery};
use openaip_os::trace::{TraceEvent, TraceKind};
use serde_json::json;

fn ev(id: &str, run: &str, ty: &str, out: serde_json::Value) -> TraceEvent {
    TraceEvent {
        id: id.to_string(),
        kind: TraceKind::Cognitive,
        event_type: ty.to_string(),
        object_refs: vec!["u1".to_string()],
        project_id: None,
        run_id: Some(run.to_string()),
        agent_id: Some("agent".to_string()),
        reasoning: None,
        input: None,
        output: Some(out),
        confidence: None,
        timestamp: "0".to_string(),
    }
}

#[tokio::test]
async fn g9a_trace_query_filters_by_run_type_and_after() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    for e in [
        ev("tr_00", "run_a", "Action", json!("Success")),
        ev("tr_01", "run_a", "PolicyCheck", json!("Allow")),
        ev("tr_02", "run_b", "Action", json!("Success")),
    ] {
        storage.append_trace(&e).await.unwrap();
    }

    // 按 run 过滤
    let a = storage
        .list_events(TraceQuery::for_run("run_a"))
        .await
        .unwrap();
    assert_eq!(a.len(), 2, "run_a 应有 2 条");
    assert!(a.iter().all(|e| e.run_id.as_deref() == Some("run_a")));

    // 按 event_type 过滤（显式 limit：Default limit=0 会被 clamp 到 1，是产品防爆保护）
    let acts = storage
        .list_events(TraceQuery {
            event_type: Some("Action".into()),
            limit: 200,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(acts.len(), 2, "Action 事件 2 条");
    assert!(acts.iter().all(|e| e.event_type == "Action"));

    // after_id 游标（WS 重连补齐语义：只取该 id 之后的事件）
    let after = storage
        .list_events(TraceQuery {
            after_id: Some("tr_00".into()),
            limit: 200,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(after.len(), 2, "tr_00 之后应剩 2 条");
    assert_eq!(after[0].id, "tr_01");
    assert_eq!(after[1].id, "tr_02");
}
