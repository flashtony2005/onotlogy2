//! P5 契约测试：World Event → Projection → Kernel World State。
//!
//! 契约文档：docs/03_架构设计/projection-v1.md（P1–P7）。
//! 覆盖：
//! - tc_pj1 事件→投影→Kernel 状态（实体 + 字段 + 业务语义边：placed/contains/owns）
//! - tc_pj2 幂等：同一事件（同 id）再次应用，状态不变（P2）
//! - tc_pj3 重放 == 正常投影：全新 Kernel 全量重放得到相同 World State（P1/P4）
//! - tc_pj4 领域规则注册：OrderPlaced → (order, "status", "placed")
//! - tc_pj5 更新/删除语义：ObjectUpdated expire 旧值、ObjectDeleted expire 实体
//! - tc_pj7a 裸写无事件 → 投影世界不含该实体（P7 反向判定：事件是唯一世界入口）
//! - tc_pj7b 事件完备 → 投影世界含全部实体（P7 正向判定）

use openaip_os::kernel::{Kernel, Scope};
use openaip_os::projection::Projection;
use openaip_os::storage::{LibSQLStorage, ObjectEngine, RelationEngine};
use openaip_os::trace::{TraceEvent, TraceKind};
use serde_json::{Value, json};

fn world_ev(id: &str, ty: &str, refs: Vec<&str>, input: Option<Value>, ts: i64) -> TraceEvent {
    TraceEvent {
        id: id.to_string(),
        kind: TraceKind::World,
        event_type: ty.to_string(),
        object_refs: refs.iter().map(|s| s.to_string()).collect(),
        run_id: None,
        agent_id: None,
        reasoning: None,
        input,
        output: None,
        confidence: None,
        timestamp: ts.to_string(),
    }
}

/// 用户拍板的闭环样例：User →(placed)→ Order →(contains)→ Product，User →(owns)→ Product。
fn demo_events() -> Vec<TraceEvent> {
    vec![
        world_ev(
            "e1",
            "ObjectCreated",
            vec!["u1"],
            Some(json!({ "type": "User", "data": { "name": "Apple", "level": "VIP" } })),
            100,
        ),
        world_ev(
            "e2",
            "ObjectCreated",
            vec!["o1"],
            Some(json!({ "type": "Order", "data": {} })),
            200,
        ),
        world_ev(
            "e3",
            "ObjectCreated",
            vec!["p1"],
            Some(json!({ "type": "Product", "data": {} })),
            300,
        ),
        world_ev(
            "e4",
            "RelationLinked",
            vec!["u1", "o1"],
            Some(json!({ "edge": "placed" })),
            400,
        ),
        world_ev(
            "e5",
            "RelationLinked",
            vec!["o1", "p1"],
            Some(json!({ "edge": "contains" })),
            500,
        ),
        world_ev(
            "e6",
            "RelationLinked",
            vec!["u1", "p1"],
            Some(json!({ "edge": "owns" })),
            600,
        ),
    ]
}

async fn append_all(storage: &LibSQLStorage, events: &[TraceEvent]) {
    for e in events {
        storage.append_trace(e).await.unwrap();
    }
}

// ── P1：事件 → 投影 → Kernel 世界（实体/字段/业务语义边）──────────────
#[tokio::test]
async fn tc_pj1_objects_and_edges_project_to_kernel() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    let events = demo_events();
    append_all(&storage, &events).await;

    let mut k = Kernel::new();
    let ctx = k.create_context(None, Scope::World, "world").unwrap();
    let mut p = Projection::new(ctx);
    let n = p.project_from_storage(&mut k, &storage).await.unwrap();
    assert_eq!(n, events.len(), "全部世界事件应被投影");

    // 实体与字段
    assert!(
        k.relations()
            .any(|r| r.source == "u1" && r.predicate == "type" && r.target == "User")
    );
    assert!(
        k.relations()
            .any(|r| r.source == "u1" && r.predicate == "name" && r.target == "Apple")
    );
    assert!(
        k.relations()
            .any(|r| r.source == "u1" && r.predicate == "level" && r.target == "VIP")
    );
    // 业务语义边（Kernel 第一次拥有业务语义关系）
    assert!(
        k.relations()
            .any(|r| r.source == "u1" && r.predicate == "placed" && r.target == "o1")
    );
    assert!(
        k.relations()
            .any(|r| r.source == "o1" && r.predicate == "contains" && r.target == "p1")
    );
    assert!(
        k.relations()
            .any(|r| r.source == "u1" && r.predicate == "owns" && r.target == "p1")
    );
}

// ── P2：幂等——同一事件（同 id）再次应用状态不变 ──────────────────────
#[test]
fn tc_pj2_same_event_twice_same_state() {
    let mut k = Kernel::new();
    let ctx = k.create_context(None, Scope::World, "world").unwrap();
    let mut p = Projection::new(ctx);
    let ev = world_ev(
        "e1",
        "ObjectCreated",
        vec!["u1"],
        Some(json!({ "type": "User", "data": { "name": "Apple" } })),
        100,
    );

    p.apply_event(&mut k, &ev).unwrap();
    let snap1 = p.state_snapshot(&k);
    p.apply_event(&mut k, &ev).unwrap();
    let snap2 = p.state_snapshot(&k);

    assert_eq!(snap1, snap2, "同一事件重复应用状态不变（P2）");
    assert_eq!(p.applied_count(), 1, "同 id 事件只应用一次");
}

// ── P1/P4：重放 == 正常投影（全新 Kernel 全量重放）────────────────────
#[tokio::test]
async fn tc_pj3_replay_equals_normal_projection() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    let events = demo_events();
    append_all(&storage, &events).await;

    // 正常投影
    let mut k1 = Kernel::new();
    let ctx1 = k1.create_context(None, Scope::World, "world").unwrap();
    let mut p1 = Projection::new(ctx1);
    p1.project_from_storage(&mut k1, &storage).await.unwrap();
    let s1 = p1.state_snapshot(&k1);

    // 重放：全新 Kernel + 同一批事件全量应用
    let mut k2 = Kernel::new();
    let ctx2 = k2.create_context(None, Scope::World, "world").unwrap();
    let mut p2 = Projection::new(ctx2);
    p2.project_events(&mut k2, &events).unwrap();
    let s2 = p2.state_snapshot(&k2);

    assert_eq!(s1, s2, "Replay(EventLog) == NormalProjection（P1/P4）");
    assert!(!s1.is_empty());
}

// ── 领域规则注册：OrderPlaced → (order, "status", "placed") ────────────
#[test]
fn tc_pj4_domain_rule_maps_event_to_relation() {
    let mut k = Kernel::new();
    let ctx = k.create_context(None, Scope::World, "world").unwrap();
    let mut p = Projection::new(ctx).with_domain_rule("OrderPlaced", "status");
    let ev = world_ev(
        "e7",
        "OrderPlaced",
        vec!["o1"],
        Some(json!({ "to": "placed" })),
        700,
    );

    p.apply_event(&mut k, &ev).unwrap();

    assert!(
        k.relations()
            .any(|r| r.source == "o1" && r.predicate == "status" && r.target == "placed"),
        "领域规则应把 OrderPlaced 投影为 (o1, status, placed)"
    );
}

// ── 更新/删除语义：expire 旧值，current 判定正确 ───────────────────────
#[test]
fn tc_pj5_update_and_delete_expire_semantics() {
    let mut k = Kernel::new();
    let ctx = k.create_context(None, Scope::World, "world").unwrap();
    let mut p = Projection::new(ctx);
    let now = k.now();

    p.apply_event(
        &mut k,
        &world_ev(
            "e1",
            "ObjectCreated",
            vec!["u1"],
            Some(json!({ "type": "User", "data": { "name": "Apple" } })),
            100,
        ),
    )
    .unwrap();
    p.apply_event(
        &mut k,
        &world_ev(
            "e2",
            "ObjectCreated",
            vec!["p1"],
            Some(json!({ "type": "Product", "data": {} })),
            200,
        ),
    )
    .unwrap();

    // 更新：旧值 expire，新值 current
    p.apply_event(
        &mut k,
        &world_ev(
            "e3",
            "ObjectUpdated",
            vec!["u1"],
            Some(json!({ "data": { "name": "Apple2" } })),
            300,
        ),
    )
    .unwrap();
    assert!(
        !k.relations().any(|r| r.source == "u1"
            && r.predicate == "name"
            && r.target == "Apple"
            && r.is_current(now)),
        "更新后旧字段值不得 current"
    );
    assert!(k.relations().any(|r| r.source == "u1"
        && r.predicate == "name"
        && r.target == "Apple2"
        && r.is_current(now)));
    assert!(
        k.relations().any(|r| r.source == "u1"
            && r.predicate == "type"
            && r.target == "User"
            && r.is_current(now)),
        "type 关系应保留"
    );

    // 删除：实体全部关系 expire
    p.apply_event(
        &mut k,
        &world_ev("e4", "ObjectDeleted", vec!["p1"], None, 400),
    )
    .unwrap();
    assert!(
        !k.relations()
            .any(|r| (r.source == "p1" || r.target == "p1") && r.is_current(now)),
        "删除后实体关系不得 current"
    );
}

// ── P7a：裸写无事件 → 投影世界不含该实体（事件是唯一世界入口）────────
#[tokio::test]
async fn tc_pj7a_bare_write_has_no_world() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    // 裸写：storage.create 不带任何事件（历史反例路径）
    storage
        .create("User", "ghost", json!({ "name": "NoEvent" }))
        .await
        .unwrap();

    let mut k = Kernel::new();
    let ctx = k.create_context(None, Scope::World, "world").unwrap();
    let mut p = Projection::new(ctx);
    let n = p.project_from_storage(&mut k, &storage).await.unwrap();

    assert_eq!(n, 0, "无事件 → 无投影");
    assert!(
        !k.relations().any(|r| r.source == "ghost"),
        "P7：裸写对象不得进入 Kernel 世界（Kernel 不接受不可追溯的 World Mutation）"
    );
}

// ── P7b：事件完备 → 投影世界含全部实体（正向判定）─────────────────────
#[tokio::test]
async fn tc_pj7b_event_complete_world_complete() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    let events = demo_events();
    append_all(&storage, &events).await;

    let mut k = Kernel::new();
    let ctx = k.create_context(None, Scope::World, "world").unwrap();
    let mut p = Projection::new(ctx);
    p.project_from_storage(&mut k, &storage).await.unwrap();

    for id in ["u1", "o1", "p1"] {
        assert!(
            k.relations()
                .any(|r| r.source == id && r.predicate == "type"),
            "P7：事件完备 → 实体 {id} 必须在投影世界"
        );
    }
}

// ── P5：WorldCommand 事务化（EveryWorldChange → Event，对象写+事件写同事务）──
use openaip_os::storage::WorldCommand;

#[tokio::test]
async fn tc_p5a_create_writes_object_and_event_transactionally() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    storage
        .apply_world(WorldCommand::CreateObject {
            object_type: "User".into(),
            id: "u1".into(),
            data: json!({ "name": "Apple", "level": "VIP" }),
        })
        .await
        .unwrap();

    // 对象落库
    let rec = storage.get("u1").await.unwrap().expect("对象应存在");
    assert_eq!(rec.r#type, "User");
    // 事件落库（契约载荷：refs=[id]，input={type,data}）
    let events = storage
        .list_events(openaip_os::storage::TraceQuery {
            event_type: Some("ObjectCreated".into()),
            limit: 200,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(events.len(), 1, "P5：create 必须伴随 ObjectCreated 事件");
    let ev = &events[0];
    assert_eq!(ev.object_refs, vec!["u1"]);
    assert_eq!(ev.input.as_ref().unwrap()["type"], "User");
    assert_eq!(ev.input.as_ref().unwrap()["data"]["name"], "Apple");
    assert_eq!(ev.run_id, None, "事实不依赖 Run（审计解耦点）");
}

#[tokio::test]
async fn tc_p5b_link_writes_relation_and_event() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    storage
        .apply_world(WorldCommand::CreateObject {
            object_type: "User".into(),
            id: "u1".into(),
            data: json!({}),
        })
        .await
        .unwrap();
    storage
        .apply_world(WorldCommand::CreateObject {
            object_type: "Order".into(),
            id: "o1".into(),
            data: json!({}),
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

    // 边落库
    let nbrs = storage.neighbors("u1").await.unwrap();
    assert!(
        nbrs.iter()
            .any(|r| r.relation == "placed" && r.target == "o1")
    );
    // 事件落库（契约载荷：refs=[source,target]，input={edge}）
    let events = storage
        .list_events(openaip_os::storage::TraceQuery {
            event_type: Some("RelationLinked".into()),
            limit: 200,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(events.len(), 1, "P5：link 必须伴随 RelationLinked 事件");
    assert_eq!(events[0].object_refs, vec!["u1", "o1"]);
    assert_eq!(events[0].input.as_ref().unwrap()["edge"], "placed");
}

#[tokio::test]
async fn tc_p5c_delete_writes_event() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    storage
        .apply_world(WorldCommand::CreateObject {
            object_type: "Product".into(),
            id: "p1".into(),
            data: json!({}),
        })
        .await
        .unwrap();
    storage
        .apply_world(WorldCommand::DeleteObject { id: "p1".into() })
        .await
        .unwrap();

    assert!(
        storage.get("p1").await.unwrap().is_none(),
        "逻辑删除后不可见"
    );
    let events = storage
        .list_events(openaip_os::storage::TraceQuery {
            event_type: Some("ObjectDeleted".into()),
            limit: 200,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(events.len(), 1, "P5：delete 必须伴随 ObjectDeleted 事件");
    assert_eq!(events[0].object_refs, vec!["p1"]);
}

// ── P5 闭环：Command → Event → Projection → Kernel（P5.4 的前置）────────
#[tokio::test]
async fn tc_p5d_commanded_world_projects_to_kernel() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    storage
        .apply_world(WorldCommand::CreateObject {
            object_type: "User".into(),
            id: "u1".into(),
            data: json!({ "name": "Apple" }),
        })
        .await
        .unwrap();
    storage
        .apply_world(WorldCommand::CreateObject {
            object_type: "Order".into(),
            id: "o1".into(),
            data: json!({}),
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

    let mut k = Kernel::new();
    let ctx = k.create_context(None, Scope::World, "world").unwrap();
    let mut p = Projection::new(ctx);
    p.project_from_storage(&mut k, &storage).await.unwrap();

    assert!(
        k.relations()
            .any(|r| r.source == "u1" && r.predicate == "type" && r.target == "User")
    );
    assert!(
        k.relations()
            .any(|r| r.source == "u1" && r.predicate == "placed" && r.target == "o1")
    );
}

// ── P5.4：per-run 投影接入——Agent 第一次观察即见 Storage 世界 ─────────────
// 契约：新建 Kernel → project_from_storage → AgentLoop；Observe 的 related
// 来自 kernel fact 层（observe.rs），故投影关系必须进入 AgentView（when 可断言）。
use openaip_os::cognitive::engine::{AgentView, FakeEngine, PlanStep};
use openaip_os::cognitive::{Agent, AgentLoop, LoopOutcome};
use openaip_os::compose::{Capability, Service, Task};
use openaip_os::kernel::effect::Reversibility;

#[tokio::test]
async fn tc_pj8_agent_observe_sees_projected_storage_world() {
    // Storage 世界：u1 是 VIP（写入路径 = WorldCommand → 事件，无裸写）
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    storage
        .apply_world(WorldCommand::CreateObject {
            object_type: "User".into(),
            id: "u1".into(),
            data: json!({ "name": "Apple", "level": "VIP" }),
        })
        .await
        .unwrap();

    // per-run 顺序与 CLI/server 一致：Kernel → 投影 → Agent（共享世界 Context）
    let mut k = Kernel::new();
    let ctx = k.create_context(None, Scope::World, "world").unwrap();
    let mut p = Projection::new(ctx);
    let n = p.project_from_storage(&mut k, &storage).await.unwrap();
    assert!(n >= 1, "P5.4：投影必须应用 storage 事件");
    let mut agent = Agent::new("retention-agent", ctx);

    // 决策依赖投影关系：仅当观察见 (u1, level, VIP) 才执行激活；否则永不行动
    let services = [Service::new("crm-service").with_capability(
        Capability::new("customer.activate", "ActivateCustomer")
            .with_reversibility(Reversibility::Reversible)
            .with_projection("status", "active")
            .requiring(&["target"]),
    )];
    let engine = FakeEngine::new(vec![PlanStep {
        capability: "customer.activate".into(),
        args: json!({ "target": "u1" }),
        when: Some(Box::new(|v: &AgentView| {
            v.has_relation("level", "VIP") && !v.has_relation("status", "active")
        })),
    }]);

    let task = Task::new("activate-vip", "激活 VIP 用户").expecting(&[("status", "active")]);
    let out = {
        let mut loop_ = AgentLoop::new(&mut k, &mut agent, &services, &engine);
        loop_.run_until_done(ctx, &task, json!({})).unwrap()
    };
    assert_eq!(
        out,
        LoopOutcome::Completed,
        "观察必须见投影关系并执行（投影失败时 when 恒假 → 永远不行动）"
    );
    // Verify：世界确实达成目标（Effect 投影写入 Kernel，不是 Agent 自称）
    assert!(
        k.relations()
            .any(|r| r.predicate == "status" && r.target == "active")
    );
}
