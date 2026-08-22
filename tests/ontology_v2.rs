//! V2.0 Ontology 语义集成（Phase 5）集成契约。
//!
//! 三层分离：① Ontology（世界是什么）→ ② Runtime（世界怎么变化）→ ③ World Model（Agent 怎么认识世界）。
//!
//! O1 ODL → RuntimeDefinition → Service 映射（D1 ABI）
//! O2 RelationEffectExecutor 真实世界变化（Effect ≠ Projection）
//! O3 RelationCompensationExecutor 撤销（D4 分离：逆序 + 关系时间窗闭合）
//! O4 EntityView 推导（D2：relations 端点聚合）
//! O5 Subgraph 图遍历检索（D3：GraphRAG 无 embedding）
//! O6 AgentViewV2 注入（D5：观察 = 实体 + 子图，认知引擎据此决策）
//! O7 ODL 端到端闭环（Ontology → Runtime → Agent → World → 重投影）

use openaip_os::abi::odl::parse_odl;
use openaip_os::cognitive::engine::{AgentView, FakeEngine, PlanStep};
use openaip_os::cognitive::observe::observe;
use openaip_os::cognitive::{Agent, AgentLoop, CognitiveEngine, LoopOutcome, Memory};
use openaip_os::compose::defs::{ir_to_runtime, ir_to_services};
use openaip_os::compose::{Capability, Service, Task, TaskExecutor};
use openaip_os::kernel::effect::{Effect, Reversibility};
use openaip_os::kernel::fiber::FiberState;
use openaip_os::kernel::relation::Relation;
use openaip_os::kernel::{Compensation, Kernel, Scope};
use openaip_os::world::{derive_entities, subgraph};
use openaip_os::world_exec::{EffectExecutor, RelationEffectExecutor};
use serde_json::json;

// ── ODL 夹具：retain（免审批）+ refund（审批 + 前置条件）──────────────
const ODL_CRM: &str = r#"
object Customer:
  properties:
    name: string
    level: string
  actions:
    retain:
      creates:
        - type: RetentionTask
          data:
            title: "客户挽留计划"
            customer: self
          link: needs_retention
    refund:
      params:
        amount: float
      updates:
        - field: balance
          value: amount
      policy:
        require_approval: true
        precondition:
          - field: amount
            op: le
            value: 1000
          - field: self.level
            op: ne
            value: "BLACKLIST"
"#;

fn crm_defs() -> Vec<openaip_os::compose::defs::RuntimeDefinition> {
    let ir = parse_odl(ODL_CRM).expect("ODL 解析");
    ir_to_runtime(&ir)
}

fn crm_services() -> Vec<Service> {
    let ir = parse_odl(ODL_CRM).expect("ODL 解析");
    ir_to_services(&ir)
}

fn spawn_agent(k: &mut Kernel) -> Agent {
    let world = k.create_context(None, Scope::World, "world").unwrap();
    Agent::new("retention-agent", world)
}

fn retain_capability(services: &[Service]) -> Capability {
    services
        .iter()
        .find_map(|s| s.capability("Customer.retain"))
        .expect("Customer.retain 能力")
        .clone()
}

// ── O1：ODL → RuntimeDefinition → Service（D1 ABI 两步编译）────────────
#[tokio::test]
async fn tc_o1_odl_to_runtime_definition_to_service() {
    let defs = crm_defs();
    assert_eq!(defs.len(), 1, "一个 object type → 一份 RuntimeDefinition");
    let def = &defs[0];
    assert_eq!(def.object_type, "Customer");

    // 能力映射：action 语义名 = 首字母大写 + 类型前缀
    let retain = def
        .capabilities
        .iter()
        .find(|c| c.name == "Customer.retain")
        .expect("retain 能力");
    assert_eq!(retain.action, "RetainCustomer");
    assert_eq!(retain.reversibility, Reversibility::Compensatable);
    assert_eq!(
        retain.effects,
        vec![openaip_os::compose::defs::EffectDefinition::Create {
            object_type: "RetentionTask".into(),
            link_edge: Some("needs_retention".into()),
        }]
    );
    assert!(retain.policy.is_none(), "免审批动作无策略");

    // 策略映射：require_approval + 前置条件（CompareOp → Cmp）
    let refund = def
        .capabilities
        .iter()
        .find(|c| c.name == "Customer.refund")
        .expect("refund 能力");
    let policy = refund.policy.as_ref().expect("refund 带策略");
    assert!(policy.require_approval);
    assert_eq!(policy.preconditions.len(), 2);
    assert_eq!(policy.preconditions[0].field, "amount");
    assert_eq!(policy.preconditions[0].op, openaip_os::compose::Cmp::Le);
    assert_eq!(policy.preconditions[0].value, json!(1000));
    assert_eq!(policy.preconditions[1].field, "self.level");
    assert_eq!(policy.preconditions[1].op, openaip_os::compose::Cmp::Ne);

    // ABI 落 V2 组合层：Service 挂载能力 + 策略
    let services = crm_services();
    let svc = &services[0];
    assert_eq!(svc.id, "Customer");
    assert!(svc.capability("Customer.retain").is_some());
    assert!(svc.capability("Customer.refund").is_some());
}

// ── O2：Effect 真实改变 World（不是投影假装）──────────────────────────
#[tokio::test]
async fn tc_o2_effect_executor_mutates_world() {
    let mut k = Kernel::new();
    let ctx = k.create_context(None, Scope::Tenant, "tenant-a").unwrap();
    let fiber = k
        .spawn(ctx, None, "retain-task", json!({}))
        .expect("spawn fiber");

    // Effect：RetainCustomer 作用于 cust-01（Create → target --link_edge--> new_id）
    let mut effect = Effect::new(
        "RetainCustomer".to_string(),
        json!({ "target": "cust-01" }),
        Reversibility::Compensatable,
        Some(Compensation {
            action: "UndoRetainCustomer".into(),
            payload: json!({}),
        }),
        k.now(),
    );
    effect.fiber = fiber;
    let result = {
        let mut ex = RelationEffectExecutor::new(&mut k, crm_defs());
        ex.execute(&effect).expect("Effect 执行成功")
    };
    assert_eq!(
        result.relations,
        vec![(
            "cust-01".to_string(),
            "needs_retention".to_string(),
            "cust-01_retaincustomer".to_string()
        )]
    );

    // 落库后 World 可见（真实关系，非 capability.projection）
    assert!(
        k.relations()
            .any(|r| r.context == ctx && r.predicate == "needs_retention")
    );

    // Update 效应：refund → target --balance--> value
    let mut effect2 = Effect::new(
        "RefundCustomer".to_string(),
        json!({ "target": "cust-01", "amount": 500 }),
        Reversibility::Compensatable,
        None,
        k.now(),
    );
    effect2.fiber = fiber;
    let result2 = {
        let mut ex = RelationEffectExecutor::new(&mut k, crm_defs());
        ex.execute(&effect2).expect("refund 执行成功")
    };
    assert_eq!(
        result2.relations,
        vec![(
            "cust-01".to_string(),
            "balance".to_string(),
            "500".to_string()
        )]
    );
    assert!(
        k.relations()
            .any(|r| r.context == ctx && r.predicate == "balance" && r.target == "500")
    );

    // 未知动作 → 执行失败（NoEffects，不产生世界变化）
    let mut bad = Effect::new(
        "ExplodePlanet".to_string(),
        json!({ "target": "earth" }),
        Reversibility::Compensatable,
        None,
        k.now(),
    );
    bad.fiber = fiber;
    {
        let mut ex = RelationEffectExecutor::new(&mut k, crm_defs());
        assert!(ex.execute(&bad).is_err());
    }
    let _ = fiber;
}

// ── O3：补偿经 RelationCompensationExecutor（撤销真实关系）────────────
#[tokio::test]
async fn tc_o3_compensation_executor_undoes_world() {
    let defs = crm_defs();
    let mut k = Kernel::new();
    let ctx = k.create_context(None, Scope::Tenant, "tenant-b").unwrap();

    let services = crm_services();
    let cap = retain_capability(&services);
    let task =
        Task::new("retain", "挽留客户").expecting(&[("needs_retention", "cust-01_retaincustomer")]);

    // ① 正向执行：Effect 真实写入 needs_retention（撤销清单进 inverse payload）
    let fiber = {
        let mut tx = TaskExecutor::new(&mut k).with_effect_defs(defs.clone());
        let f = tx.spawn(ctx, &task, json!({})).unwrap();
        tx.step(f, &cap, json!({ "target": "cust-01" })).unwrap();
        f
    };
    assert!(
        k.relations()
            .any(|r| r.predicate == "needs_retention" && r.is_current(k.now())),
        "正向执行后关系应存在"
    );
    assert_eq!(
        k.relations()
            .filter(|r| r.predicate == "needs_retention" && r.is_current(k.now()))
            .count(),
        1,
        "正向 Effect 只能由 RelationEffectExecutor 应用一次"
    );

    // ② 下游失败 → 置 Failed → 逆序补偿（D4：RelationCompensationExecutor 闭合时间窗）
    k.advance(fiber, FiberState::Failed).unwrap();
    {
        let mut tx = TaskExecutor::new(&mut k).with_relation_compensation();
        tx.compensate(fiber).expect("补偿成功");
    }
    assert_eq!(
        k.fiber(fiber).map(|f| f.state),
        Some(FiberState::Compensated)
    );
    // 世界回滚：needs_retention 时间窗已闭合
    assert!(
        !k.relations()
            .any(|r| r.predicate == "needs_retention" && r.is_current(k.now())),
        "补偿后关系应已撤销"
    );
}

// ── O4：EntityView 推导（D2：relations 端点聚合，不依赖 V1 storage）────
#[tokio::test]
async fn tc_o4_entity_view_derivation() {
    let mut k = Kernel::new();
    let ctx = k.create_context(None, Scope::World, "world").unwrap();
    let t0 = k.now();
    k.relate(Relation::new(ctx, "user-001", "status", "active", t0))
        .unwrap();
    k.relate(Relation::new(ctx, "user-001", "owns", "order-001", t0))
        .unwrap();
    // amount=100 数值：target 仍文本化（图边），元数据 metadata 保留 Number(100)——
    // 与投影行为一致（src/projection/mod.rs 写入 metadata 还原数值类型）。
    let mut amount_rel = Relation::new(ctx, "order-001", "amount", "100", t0);
    amount_rel.metadata = json!(100);
    k.relate(amount_rel).unwrap();

    let entities = derive_entities(&k, ctx);
    let user = entities
        .iter()
        .find(|e| e.id == "user-001")
        .expect("user 实体");
    assert_eq!(user.entity_type, "user", "类型由 id 前缀推导");
    assert_eq!(user.outgoing.len(), 2);
    assert!(
        user.outgoing
            .contains(&("status".to_string(), json!("active")))
    );
    assert!(
        user.outgoing
            .contains(&("owns".to_string(), json!("order-001")))
    );

    let order = entities
        .iter()
        .find(|e| e.id == "order-001")
        .expect("order 实体");
    assert_eq!(order.entity_type, "order");
    assert_eq!(
        order.incoming,
        vec![("user-001".to_string(), json!("owns"))]
    );
    // 数值字段 amount=100 保留为 Number（不再文本化为 "100"）—— 类型还原修复验证
    assert_eq!(order.outgoing, vec![("amount".to_string(), json!(100))]);
}

// ── O5：Subgraph 图遍历（D3：GraphRAG = BFS，无 embedding）─────────────
#[tokio::test]
async fn tc_o5_subgraph_traversal() {
    let mut k = Kernel::new();
    let ctx = k.create_context(None, Scope::World, "world").unwrap();
    let t0 = k.now();
    k.relate(Relation::new(ctx, "a", "r1", "b", t0)).unwrap();
    k.relate(Relation::new(ctx, "b", "r2", "c", t0)).unwrap();
    k.relate(Relation::new(ctx, "c", "r3", "d", t0)).unwrap();
    k.relate(Relation::new(ctx, "x", "isolated", "y", t0))
        .unwrap();

    // 2 跳：a → b → c（d 超出深度，isolated 不可达）
    let sg = subgraph(&k, ctx, "a", 2);
    assert_eq!(sg.seed, "a");
    assert_eq!(sg.relations.len(), 2);
    assert!(
        sg.relations
            .contains(&("a".into(), "r1".into(), "b".into()))
    );
    assert!(
        sg.relations
            .contains(&("b".into(), "r2".into(), "c".into()))
    );
    let ids: Vec<&str> = sg.entities.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, vec!["a", "b", "c"]);
    assert!(!ids.contains(&"d"), "超出深度不进入子图");
    assert!(!ids.contains(&"x"), "不可达节点不进入子图");

    // 深度 0：仅 seed
    let sg0 = subgraph(&k, ctx, "a", 0);
    assert_eq!(sg0.entities.len(), 1);
    assert!(sg0.relations.is_empty());
}

// ── O6：AgentViewV2 注入（观察 = 实体 + 子图；认知引擎据此决策）───────
#[tokio::test]
async fn tc_o6_agent_view_v2_injection() {
    let mut k = Kernel::new();
    let ctx = k.create_context(None, Scope::World, "world").unwrap();
    let t0 = k.now();
    k.relate(Relation::new(
        ctx,
        "cust-01",
        "needs_retention",
        "cust-01_retaincustomer",
        t0,
    ))
    .unwrap();
    k.relate(Relation::new(ctx, "cust-01", "level", "VIP", t0))
        .unwrap();
    let _agent = Agent::new("retention-agent", ctx);
    let memory = Memory::new();
    let caps = vec!["Customer.retain".to_string()];

    let view = observe(&k, ctx, "reduce churn", &memory, &caps, None);
    // 实体视图注入
    assert!(
        view.entities.iter().any(|e| e.id == "cust-01"),
        "AgentView.entities 应含 cust-01"
    );
    // 语义子图注入（seed = 首条关系 source）
    let sg = view.subgraph.as_ref().expect("subgraph 注入");
    assert_eq!(sg.seed, "cust-01");
    assert!(
        sg.relations.contains(&(
            "cust-01".into(),
            "needs_retention".into(),
            "cust-01_retaincustomer".into()
        )),
        "子图应含 needs_retention 边"
    );

    // 认知引擎基于语义视图决策：when 谓词读 subgraph（证明决策输入是 World Model 投影）
    let engine = FakeEngine::new(vec![PlanStep {
        capability: "Customer.retain".into(),
        args: json!({ "target": "cust-01" }),
        when: Some(Box::new(|v: &AgentView| {
            v.subgraph.as_ref().is_some_and(|sg| {
                sg.entities.iter().any(|e| e.id == "cust-01")
                    && sg.relations.iter().any(|(_, p, _)| p == "needs_retention")
            })
        })),
    }]);
    let d = engine
        .decide(&view)
        .expect("decide")
        .expect("子图条件满足 → 有决策");
    assert_eq!(d.capability, "Customer.retain");
}

// ── O7：ODL 端到端闭环（Ontology → Runtime → Agent → World → 重投影）──
#[tokio::test]
async fn tc_o7_odl_end_to_end_closed_loop() {
    let defs = crm_defs();
    let services = crm_services();
    let mut k = Kernel::new();
    let mut agent = spawn_agent(&mut k);
    let ctx = agent.context;

    // 认知引擎：目标达成前持续行动（D5 目标函数）——基于 World Model 观察防重
    let engine = FakeEngine::new(vec![PlanStep {
        capability: "Customer.retain".into(),
        args: json!({ "target": "cust-01" }),
        when: Some(Box::new(|v: &AgentView| {
            !v.has_relation("needs_retention", "cust-01_retaincustomer")
        })),
    }]);

    let task = Task::new("retain", "挽留高危客户")
        .expecting(&[("needs_retention", "cust-01_retaincustomer")]);
    let out = {
        let mut loop_ =
            AgentLoop::new(&mut k, &mut agent, &services, &engine).with_effect_defs(defs.clone());
        loop_.run_until_done(ctx, &task, json!({})).unwrap()
    };
    assert_eq!(out, LoopOutcome::Completed);

    // ① World 真实变化（RelationEffectExecutor 落库，非 projection）
    assert!(
        k.relations().any(|r| r.predicate == "needs_retention"
            && r.target == "cust-01_retaincustomer"
            && r.is_current(k.now())),
        "Effect 应真实写入 World"
    );

    // ② World Model 重投影：实体 + 子图回喂认知层（D5：Agent 重新观察世界）
    let memory = Memory::new();
    let view = observe(&k, ctx, "reduce churn", &memory, &[], None);
    let sg = view.subgraph.as_ref().expect("重投影子图");
    assert!(
        sg.relations
            .iter()
            .any(|(s, p, _)| s == "cust-01" && p == "needs_retention"),
        "重投影后子图应含 needs_retention"
    );
    assert!(
        view.entities
            .iter()
            .any(|e| e.id == "cust-01_retaincustomer"),
        "重投影后应推导出 RetentionTask 实体"
    );

    // ③ 引擎看到达成状态 → 无更多决策（目标函数收敛）
    let d = engine.decide(&view).expect("decide");
    assert!(d.is_none(), "世界已达成 → 引擎不再决策");
}
