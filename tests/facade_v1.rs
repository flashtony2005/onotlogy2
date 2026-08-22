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
use openaip_os::worker::process::{
    ExecutionContext, ExecutionWorkspace, ProcessWorker, WorkspaceMode,
};
use std::sync::Arc;

fn isolated_context() -> ExecutionContext {
    ExecutionContext {
        workspace: ExecutionWorkspace {
            root: "/tmp".into(),
            mode: WorkspaceMode::Isolated,
        },
        cwd: None,
        env: Default::default(),
        timeout_ms: None,
    }
}

fn sh_adapter(capabilities: &[&str]) -> ProcessAdapter {
    let mut m = WorkerManifest::new("sh-agent", "agent").with_capabilities(capabilities);
    m.quality = 0.9;
    m.risk = 2;
    ProcessAdapter::new("sh-agent", ProcessWorker::new(m, "/bin/sh"))
}

/// multi_thread runtime + block_in_place：Facade 同步 API 内部 block_on storage。
/// T2b-2 异步契约轮询：等后台 spawn_blocking 任务终局（fabric.task_{completed,failed,cancelled}）。
/// 按 correlation_id 在事件 input 中匹配；超时 panic。返回终局事件类型。
async fn wait_fabric_done(storage: &Arc<LibSQLStorage>, cid: &str) -> String {
    for _ in 0..300 {
        let evs = storage
            .list_events(openaip_os::storage::TraceQuery {
                limit: 100,
                ..Default::default()
            })
            .await
            .unwrap();
        for e in &evs {
            if let Some(input) = &e.input
                && input.get("correlation_id").and_then(|c| c.as_str()) == Some(cid)
                && matches!(
                    e.event_type.as_str(),
                    "fabric.task_completed" | "fabric.task_failed" | "fabric.task_cancelled"
                )
            {
                return e.event_type.clone();
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("fabric 后台任务未在超时内终局（cid={cid}）");
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
            project_id: None,
        })
        .await
        .unwrap();
    storage
        .apply_world(WorldCommand::CreateObject {
            object_type: "Order".into(),
            id: "o1".into(),
            data: serde_json::json!({}),
            project_id: None,
        })
        .await
        .unwrap();
    storage
        .apply_world(WorldCommand::LinkRelation {
            source: "u1".into(),
            edge: "placed".into(),
            target: "o1".into(),
            project_id: None,
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
                project_id: None,
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
        // fabric.submit：T2b-2 异步契约——立即返 running + correlation_id（子进程后台执行）
        let sub = facade
            .fabric_submit(FabricRequest {
                session_id: "sess_g3".into(),
                sender: "dsh".into(),
                intent: "run_check".into(),
                payload: serde_json::json!({
                    "args": ["-c", "echo ok"],
                    "object": "order-001",
                }),
                execution_context: Some(isolated_context()),
                project_id: None,
            })
            .unwrap();
        assert_eq!(
            sub.result.payload["status"], "running",
            "异步契约：立即返 running"
        );
        assert_eq!(sub.result.sender, "dsh", "running 消息来自请求方");
        assert!(sub.result.correlation_id.is_some());
        sub
    });

    // T2b-2 异步契约：轮询后台任务终局（spawn_blocking 执行 /bin/sh → fabric.task_completed）
    let cid = submission.result.correlation_id.clone().unwrap();
    let done = wait_fabric_done(&storage, &cid).await;
    assert_eq!(done, "fabric.task_completed", "后台任务应终局为完成");

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
            project_id: None,
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
            execution_context: None,
            project_id: None,
        });
        assert!(f.is_err(), "未知意图必须 fail-closed（Facade 语义透出）");
        // trait 对象化：Transport 只见 OpenAipFacade（编译级：内部实现不可达）
        let trait_obj: &dyn OpenAipFacade = &facade;
        let w2 = trait_obj.world_query(WorldQuery::default()).unwrap();
        assert_eq!(w2.total, w.total);
    });
}

// ── Phase 8：Project Scope——项目维度的世界模型 ────────────────────────
// tc_p1 project.create/list（项目注册表）
// tc_p2 事件项目归属 + 世界按项目隔离（对象/事件带 project_id → 项目世界）
// tc_p3 fabric.submit(project) → 事件带项目 → 项目世界投影可见（提交即项目世界）
use openaip_os::facade::{ProjectCreate, ProjectView};

#[tokio::test(flavor = "multi_thread")]
async fn tc_p1_project_create_and_list() {
    let storage = Arc::new(LibSQLStorage::open(":memory:").await.unwrap());
    tokio::task::block_in_place(|| {
        let facade = Facade::new(
            storage.clone(),
            tokio::runtime::Handle::current(),
            FabricRouter::new(),
            vec![],
            &[],
        )
        .unwrap();
        // 初始为空
        assert_eq!(facade.project_list().unwrap().len(), 0);
        // 创建项目
        let p = facade
            .project_create(ProjectCreate {
                id: "openaip-os".into(),
                name: "OpenAIP-OS 工程".into(),
                description: "第一个项目：本体论管理本工程".into(),
                ontology: "object Module { ... }".into(),
            })
            .unwrap();
        assert_eq!(p.id, "openaip-os");
        assert_eq!(p.name, "OpenAIP-OS 工程");
        assert_eq!(p.status, "active");
        // 列表可见
        let list: Vec<ProjectView> = facade.project_list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "openaip-os");
        assert_eq!(list[0].created_at.len(), 19);
    });
}

#[tokio::test(flavor = "multi_thread")]
async fn tc_p2_world_is_project_scoped() {
    let storage = Arc::new(LibSQLStorage::open(":memory:").await.unwrap());
    // 项目 p1 世界：u1 对象
    storage
        .apply_world(WorldCommand::CreateObject {
            object_type: "User".into(),
            id: "u1".into(),
            data: serde_json::json!({ "name": "Apple", "level": "VIP" }),
            project_id: Some("p1".into()),
        })
        .await
        .unwrap();
    // 项目 p2 世界：u2 对象（隔离）
    storage
        .apply_world(WorldCommand::CreateObject {
            object_type: "User".into(),
            id: "u2".into(),
            data: serde_json::json!({ "name": "Banana" }),
            project_id: Some("p2".into()),
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
        // p1 世界只见 u1
        let w1 = facade
            .world_query(WorldQuery {
                project_id: Some("p1".into()),
                ..Default::default()
            })
            .unwrap();
        assert!(
            w1.relations.iter().any(|r| r.source == "u1"),
            "p1 世界应含 u1: {:?}",
            w1.relations
        );
        assert!(
            !w1.relations.iter().any(|r| r.source == "u2"),
            "p1 世界不得含 u2（项目隔离）"
        );
        // p2 世界只见 u2
        let w2 = facade
            .world_query(WorldQuery {
                project_id: Some("p2".into()),
                ..Default::default()
            })
            .unwrap();
        assert!(w2.relations.iter().any(|r| r.source == "u2"));
        assert!(!w2.relations.iter().any(|r| r.source == "u1"));
        // 全局（None）见全部
        let all = facade.world_query(WorldQuery::default()).unwrap();
        assert!(all.relations.iter().any(|r| r.source == "u1"));
        assert!(all.relations.iter().any(|r| r.source == "u2"));
    });
    // 存储层：事件 project 归属正确（ObjectCreated 非审计域，用 list_events 验证）
    let evs = storage
        .list_events(openaip_os::storage::TraceQuery {
            project_id: Some("p1".into()),
            limit: 10,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(evs.len(), 1);
    assert_eq!(evs[0].object_refs, vec!["u1"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn tc_p3_fabric_submit_lands_in_project_world() {
    let storage = Arc::new(LibSQLStorage::open(":memory:").await.unwrap());
    let mut router = FabricRouter::new();
    router.register_intent("run_check", "shell.execute");
    router.register_agent("shell.execute", "sh-agent", 1);

    let sub = tokio::task::block_in_place(|| {
        let facade = Facade::new(
            storage.clone(),
            tokio::runtime::Handle::current(),
            router,
            vec![Arc::new(sh_adapter(&["shell.execute"]))],
            &[("fabric.task_completed", "fabric.status")],
        )
        .unwrap();
        let s = facade
            .fabric_submit(FabricRequest {
                session_id: "sess_p3".into(),
                sender: "dsh".into(),
                intent: "run_check".into(),
                payload: serde_json::json!({
                    "args": ["-c", "echo ok"],
                    "object": "order-p3",
                }),
                execution_context: Some(isolated_context()),
                project_id: Some("openaip-os".into()),
            })
            .unwrap();
        assert_eq!(
            s.result.payload["status"], "running",
            "异步契约：立即返 running"
        );
        assert!(s.result.correlation_id.is_some());
        s
    });

    // T2b-2 异步契约：轮询后台任务终局
    let cid = sub.result.correlation_id.clone().unwrap();
    let done = wait_fabric_done(&storage, &cid).await;
    assert_eq!(done, "fabric.task_completed", "后台任务应终局为完成");

    // 事件带项目归属
    let evs = storage
        .list_events(openaip_os::storage::TraceQuery {
            event_type: Some("fabric.task_completed".into()),
            limit: 10,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(evs.len(), 1);
    assert_eq!(evs[0].project_id.as_deref(), Some("openaip-os"));

    // 项目世界投影可见（提交即项目世界）
    tokio::task::block_in_place(|| {
        let facade = Facade::new(
            storage.clone(),
            tokio::runtime::Handle::current(),
            FabricRouter::new(),
            vec![],
            &[("fabric.task_completed", "fabric.status")],
        )
        .unwrap();
        let w = facade
            .world_query(WorldQuery {
                project_id: Some("openaip-os".into()),
                object_ref: Some("order-p3".into()),
                ..Default::default()
            })
            .unwrap();
        assert!(
            w.relations
                .iter()
                .any(|r| r.predicate == "fabric.status" && r.target == "completed"),
            "fabric 提交必须进入项目世界: {:?}",
            w.relations
        );
        // 全局世界（None）也能看到（None=全部项目）
        let all = facade.world_query(WorldQuery::default()).unwrap();
        assert!(all.relations.iter().any(|r| r.source == "order-p3"));
    });
}

// ── Phase 8 遗留边界：工程健康度 → 项目世界（CheckRun / Commit）────────
// tc_p4 record_check_run：make check 结果 → CheckRun 对象 + root contains 关系
//     → 项目世界可查（状态时间线，幂等重跑不爆错）
// tc_p5 record_commit：git 提交 → Commit 对象 → 项目世界可查
use openaip_os::project_seed::{
    project_add_object, record_check_run, record_commit, seed_dependencies, seed_openaip_os,
};

#[tokio::test(flavor = "multi_thread")]
async fn tc_p4_check_run_lands_in_project_world() {
    let storage = Arc::new(LibSQLStorage::open(":memory:").await.unwrap());
    // seed 工程结构（模块/文件）+ 记录一次 check
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current()
            .block_on(seed_openaip_os(&storage, "."))
            .unwrap();
    });
    record_check_run(&storage, "passed", 350, 0, 350, 178665)
        .await
        .unwrap();
    record_check_run(&storage, "failed", 100, 2, 102, 5000)
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
        // 项目世界：CheckRun 对象（type 关系）+ root contains 边
        let w = facade
            .world_query(WorldQuery {
                project_id: Some("openaip-os".into()),
                predicate: Some("type".into()),
                ..Default::default()
            })
            .unwrap();
        let check_runs = w
            .relations
            .iter()
            .filter(|r| r.target == "CheckRun")
            .count();
        assert_eq!(
            check_runs,
            2,
            "两次 check → 两条 CheckRun 时间线: {:?}",
            w.relations.iter().map(|r| &r.target).collect::<Vec<_>>()
        );
        // 对象载荷（passed/total 字段关系）
        let w2 = facade
            .world_query(WorldQuery {
                project_id: Some("openaip-os".into()),
                predicate: Some("passed".into()),
                ..Default::default()
            })
            .unwrap();
        assert!(
            w2.relations.iter().any(|r| r.target == "350"),
            "CheckRun 的 passed=350 应在世界: {:?}",
            w2.relations.iter().map(|r| &r.target).collect::<Vec<_>>()
        );
    });
    // 幂等：重复记录不爆错（同 check id 时间戳唯一——新对象；重复 seed 已安全）
    let _ = record_check_run(&storage, "passed", 350, 0, 350, 1).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn tc_p5_commit_lands_in_project_world() {
    let storage = Arc::new(LibSQLStorage::open(":memory:").await.unwrap());
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current()
            .block_on(seed_openaip_os(&storage, "."))
            .unwrap();
    });
    record_commit(
        &storage,
        "abc123",
        "feat(x): 示例提交",
        "mason",
        "2026-08-17T01:00:00+08:00",
    )
    .await
    .unwrap();
    // 重复同 hash：幂等（跳过已存在）
    record_commit(
        &storage,
        "abc123",
        "feat(x): 示例提交",
        "mason",
        "2026-08-17T01:00:00+08:00",
    )
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
        let w = facade
            .world_query(WorldQuery {
                project_id: Some("openaip-os".into()),
                predicate: Some("hash".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            w.relations.iter().filter(|r| r.target == "abc123").count(),
            1,
            "同 hash 幂等——世界只有一条 Commit"
        );
        let msg = facade
            .world_query(WorldQuery {
                project_id: Some("openaip-os".into()),
                predicate: Some("message".into()),
                ..Default::default()
            })
            .unwrap();
        assert!(
            msg.relations
                .iter()
                .any(|r| r.target == "feat(x): 示例提交"),
            "Commit message 应在世界"
        );
    });
}

// ── Phase 8：项目世界对象写入（facade project event 语义）────────
// tc_p6 project_add_object：给项目世界加对象（Task）+ 可选 root contains →
//     项目世界可查（对象 + 关系都带 project 归属）；重复 id 幂等不爆错
#[tokio::test(flavor = "multi_thread")]
async fn tc_p6_project_event_lands_in_project_world() {
    let storage = Arc::new(LibSQLStorage::open(":memory:").await.unwrap());
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current()
            .block_on(seed_openaip_os(&storage, "."))
            .unwrap();
    });
    // 给项目世界加一个 Task 对象 + root contains 关系
    project_add_object(
        &storage,
        "Task",
        "task-1",
        "openaip-os",
        &serde_json::json!({ "title": "demo task", "status": "open" }),
        Some("contains"),
    )
    .await
    .unwrap();
    // 二次添加同 id：幂等（跳过已存在，不爆错）
    project_add_object(
        &storage,
        "Task",
        "task-1",
        "openaip-os",
        &serde_json::json!({ "title": "demo task", "status": "open" }),
        Some("contains"),
    )
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
        // Task 对象进入项目世界（type 关系）
        let w = facade
            .world_query(WorldQuery {
                project_id: Some("openaip-os".into()),
                predicate: Some("type".into()),
                ..Default::default()
            })
            .unwrap();
        assert!(
            w.relations
                .iter()
                .any(|r| r.source == "task-1" && r.target == "Task"),
            "Task 对象应进入 openaip-os 项目世界: {:?}",
            w.relations
        );
        // root --contains--> task-1（--edge 关系）
        let c = facade
            .world_query(WorldQuery {
                project_id: Some("openaip-os".into()),
                predicate: Some("contains".into()),
                ..Default::default()
            })
            .unwrap();
        assert!(
            c.relations
                .iter()
                .any(|r| r.source == "module:root" && r.target == "task-1"),
            "root --contains--> task-1 应存在: {:?}",
            c.relations
        );
    });
}

// ── Phase 8：依赖图 seed（crates 依赖 → depends_on 关系）────────
// tc_p7 seed_dependencies：扫描 crates/*/Cargo.toml 的 oaip-* 依赖 →
//      module --depends_on--> module 进入项目世界（幂等：重复边 WB2007 跳过）
#[tokio::test(flavor = "multi_thread")]
async fn tc_p7_dependency_graph_relations() {
    let storage = Arc::new(LibSQLStorage::open(":memory:").await.unwrap());
    let (n1, n2) = tokio::task::block_in_place(|| {
        let h = tokio::runtime::Handle::current();
        h.block_on(seed_openaip_os(&storage, ".")).unwrap();
        // 幂等：连续两次 seed 计数一致（重复边被跳过）
        let n1 = h.block_on(seed_dependencies(&storage, ".")).unwrap();
        let n2 = h.block_on(seed_dependencies(&storage, ".")).unwrap();
        (n1, n2)
    });
    assert_eq!(n1, n2, "依赖 seed 幂等：重复边被跳过，计数一致");
    assert!(n1 >= 1, "crates 内存在 oaip-* 依赖: {n1}");

    tokio::task::block_in_place(|| {
        let facade = Facade::new(
            storage.clone(),
            tokio::runtime::Handle::current(),
            FabricRouter::new(),
            vec![],
            &[],
        )
        .unwrap();
        let w = facade
            .world_query(WorldQuery {
                project_id: Some("openaip-os".into()),
                predicate: Some("depends_on".into()),
                ..Default::default()
            })
            .unwrap();
        assert!(
            !w.relations.is_empty(),
            "depends_on 边进入项目世界: {:?}",
            w.relations
        );
        assert!(
            w.relations
                .iter()
                .all(|r| r.source.starts_with("module:") && r.target.starts_with("module:")),
            "depends_on 两端必须是模块: {:?}",
            w.relations
        );
    });
}
