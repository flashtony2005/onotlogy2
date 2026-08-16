//! Facade V1.0 契约测试：OpenAIP Public Semantic Facade（三语义域）。
//!
//! 契约：docs/03_架构设计/facade-v1.md
//! - tc_g1 world_query：事件 → 投影 → 世界视图（无过滤 / object_ref / predicate）
//! - tc_g2 audit_query：审计语义域（approval.* / fabric.*）+ 前缀过滤
//! - tc_g3 fabric_submit 闭环：submit → Router → Adapter → 事件落库 →
//!   再 world_query 可见（提交即世界）
//! - tc_g4 单一语义源：Facade 是唯一入口（行为级：三个查询走同一 Facade，
//!   内部实现不可达）

use openaip_os::fabric::adapters::ProcessAdapter;
use openaip_os::fabric::router::FabricRouter;
use openaip_os::facade::{AuditQuery, FabricRequest, Facade, OpenAipFacade, WorldQuery};
use openaip_os::storage::{LibSQLStorage, WorldCommand};
use openaip_os::worker::manifest::WorkerManifest;
use openaip_os::worker::process::ProcessWorker;
use std::sync::Arc;

fn sh_adapter(capabilities: &[&str]) -> ProcessAdapter {
    let mut m = WorkerManifest::new("sh-agent", "agent").with_capabilities(capabilities);
    m.quality = 0.9;
    m.risk = 2;
    ProcessAdapter::new("sh-agent", ProcessWorker::new(m, "/bin/sh"))
}

/// multi_thread runtime + block_in_place：Facade 同步 API 内部 block_on storage。
#[tokio::test(flavor = "multi_thread")]
async fn tc_g1_world_query_projects_events_to_view() {
    let storage = Arc::new(LibSQLStorage::open(":memory:").await.unwrap());
    storage
        .apply_world(WorldCommand::CreateObject {
            object_type: "User".into(),
            id: "u1".into(),
            data: serde_json::json!({ "name": "Apple", "level": "VIP" }),
        })
        .await
        .unwrap();
    storage
        .apply_world(WorldCommand::CreateObject {
            object_type: "Order".into(),
            id: "o1".into(),
            data: serde_json::json!({}),
        })
        .await
        .unwrap();
    storage
        .apply_world(WorldCommand::LinkRelation {
            source: "u1".into(),
            edge: "placed".into(),
            target: "o1".into(),
        })
        .await
        .unwrap();

    let out = tokio::task::block_in_place(|| {
        let facade = Facade::new(
            storage.clone(),
            tokio::runtime::Handle::current(),
            FabricRouter::new(),
            vec![],
            &[],
        )
        .unwrap();
        // 无过滤：全量世界视图
        let all = facade.world_query(WorldQuery::default()).unwrap();
        assert!(
            all.total >= 5,
            "u1 实体(type/name/level) + o1(type) + placed 边，全量视图: {}",
            all.total
        );
        assert!(
            all.relations
                .iter()
                .any(|r| r.source == "u1" && r.predicate == "level" && r.target == "VIP")
        );
        // object_ref 过滤
        let o = facade
            .world_query(WorldQuery {
                object_ref: Some("o1".into()),
                ..Default::default()
            })
            .unwrap();
        assert!(
            o.relations
                .iter()
                .all(|r| r.source == "o1" || r.target == "o1")
        );
        assert!(
            o.relations
                .iter()
                .any(|r| r.predicate == "type" && r.target == "Order")
        );
        // predicate 过滤
        let p = facade
            .world_query(WorldQuery {
                predicate: Some("placed".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(p.total, 1);
        assert_eq!(p.relations[0].source, "u1");
        assert_eq!(p.relations[0].target, "o1");
        // limit
        let l = facade
            .world_query(WorldQuery {
                limit: 2,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(l.relations.len(), 2);
        all
    });
    assert!(out.total >= 5);
}

#[tokio::test(flavor = "multi_thread")]
async fn tc_g2_audit_query_filters_audit_domain() {
    let storage = Arc::new(LibSQLStorage::open(":memory:").await.unwrap());
    // 造审计事件（通信黑匣子 + 审批域 + 非审计事件）
    for (ty, input) in [
        (
            "fabric.task_completed",
            serde_json::json!({ "to": "completed" }),
        ),
        (
            "approval.requested",
            serde_json::json!({ "capability": "shell.execute" }),
        ),
        ("evolution.gate_approved", serde_json::json!({})),
        ("effect.committed", serde_json::json!({})), // 非审计域
    ] {
        storage
            .append_trace(&openaip_os::trace::TraceEvent {
                id: format!("ev_{ty}"),
                kind: openaip_os::trace::TraceKind::Cognitive,
                event_type: ty.into(),
                object_refs: vec!["world".into()],
                run_id: Some("run_g2".into()),
                agent_id: Some("retention-agent".into()),
                reasoning: None,
                input: Some(input),
                output: None,
                confidence: None,
                timestamp: "100".into(),
            })
            .await
            .unwrap();
    }

    let view = tokio::task::block_in_place(|| {
        let facade = Facade::new(
            storage.clone(),
            tokio::runtime::Handle::current(),
            FabricRouter::new(),
            vec![],
            &[],
        )
        .unwrap();
        // 审计语义域（默认全量）：非审计事件被过滤
        let all = facade.audit_query(AuditQuery::default()).unwrap();
        let types: Vec<&str> = all.events.iter().map(|e| e.event_type.as_str()).collect();
        assert_eq!(types.len(), 3, "effect.committed 不属于审计域: {types:?}");
        assert!(types.contains(&"fabric.task_completed"));
        assert!(types.contains(&"approval.requested"));
        assert!(types.contains(&"evolution.gate_approved"));
        // 前缀过滤："approval." 匹配全部 approval.*
        let appr = facade
            .audit_query(AuditQuery {
                event_type: Some("approval.".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(appr.total, 1);
        assert_eq!(appr.events[0].event_type, "approval.requested");
        assert_eq!(appr.events[0].refs, vec!["world"]);
        // run 过滤
        let run = facade
            .audit_query(AuditQuery {
                run_id: Some("run_g2".into()),
                event_type: Some("fabric.".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(run.total, 1);
        all
    });
    assert_eq!(view.total, 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn tc_g3_fabric_submit_closed_loop_world_visible() {
    let storage = Arc::new(LibSQLStorage::open(":memory:").await.unwrap());
    let mut router = FabricRouter::new();
    router.register_intent("run_check", "shell.execute");
    router.register_agent("shell.execute", "sh-agent", 1);

    let submission = tokio::task::block_in_place(|| {
        let facade = Facade::new(
            storage.clone(),
            tokio::runtime::Handle::current(),
            router,
            vec![Arc::new(sh_adapter(&["shell.execute"]))],
            // 领域规则：fabric.task_completed → (refs[0], fabric.status, input.to)
            &[("fabric.task_completed", "fabric.status")],
        )
        .unwrap();
        // fabric.submit：经 Router → Adapter（/bin/sh 真实进程）→ task.result
        let sub = facade
            .fabric_submit(FabricRequest {
                session_id: "sess_g3".into(),
                sender: "dsh".into(),
                intent: "run_check".into(),
                payload: serde_json::json!({
                    "args": ["-c", "echo ok"],
                    "object": "order-001",
                }),
            })
            .unwrap();
        assert_eq!(sub.result.payload["status"], "completed");
        assert_eq!(sub.result.sender, "sh-agent");
        assert!(sub.result.correlation_id.is_some());
        sub
    });
    assert_eq!(submission.result.payload["status"], "completed");

    // 提交即世界：事件已落库 → 再 world_query 可见（领域规则投影）
    let view = tokio::task::block_in_place(|| {
        let facade = Facade::new(
            storage.clone(),
            tokio::runtime::Handle::current(),
            FabricRouter::new(),
            vec![],
            &[("fabric.task_completed", "fabric.status")],
        )
        .unwrap();
        let v = facade
            .world_query(WorldQuery {
                object_ref: Some("order-001".into()),
                ..Default::default()
            })
            .unwrap();
        assert!(
            v.relations
                .iter()
                .any(|r| r.predicate == "fabric.status" && r.target == "completed"),
            "fabric 提交结果必须进入世界状态（P5 单向流）：{:?}",
            v.relations
        );
        // audit 复核：通信黑匣子可见
        let a = facade
            .audit_query(AuditQuery {
                event_type: Some("fabric.".into()),
                ..Default::default()
            })
            .unwrap();
        assert!(
            a.events
                .iter()
                .any(|e| e.event_type == "fabric.task_completed"),
            "审计必须包含通信事件"
        );
        v
    });
    assert!(view.total >= 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn tc_g4_single_semantic_source_all_queries_through_facade() {
    let storage = Arc::new(LibSQLStorage::open(":memory:").await.unwrap());
    storage
        .apply_world(WorldCommand::CreateObject {
            object_type: "User".into(),
            id: "u1".into(),
            data: serde_json::json!({ "name": "Apple" }),
        })
        .await
        .unwrap();

    tokio::task::block_in_place(|| {
        let facade = Facade::new(
            storage.clone(),
            tokio::runtime::Handle::current(),
            FabricRouter::new(),
            vec![],
            &[],
        )
        .unwrap();
        // 三个语义域走同一 Facade（Transport 只做参数翻译）
        let w = facade.world_query(WorldQuery::default()).unwrap();
        assert!(w.total >= 2);
        let a = facade.audit_query(AuditQuery::default()).unwrap();
        assert_eq!(a.total, 0, "空事件库无审计");
        // Fabric 域：未注册提供方 → FacadeError（fail-closed 语义透出）
        let f = facade.fabric_submit(FabricRequest {
            session_id: "sess_g4".into(),
            sender: "dsh".into(),
            intent: "nothing".into(),
            payload: serde_json::json!({}),
        });
        assert!(f.is_err(), "未知意图必须 fail-closed（Facade 语义透出）");
        // trait 对象化：Transport 只见 OpenAipFacade（编译级：内部实现不可达）
        let trait_obj: &dyn OpenAipFacade = &facade;
        let w2 = trait_obj.world_query(WorldQuery::default()).unwrap();
        assert_eq!(w2.total, w.total);
    });
}
