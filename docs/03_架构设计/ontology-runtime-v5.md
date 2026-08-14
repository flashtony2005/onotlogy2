# Ontology → Runtime → World Model 语义集成（Phase 5，设计定稿）

| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-03-V5-ONTOLOGY |
| 状态 | **设计定稿**（2026-08-14 评审拍板） |
| 定位 | Ontology Semantic Integration & World Model Projection |

## 0. 一句话交付标准

> 给定一份 V1 ODL，V2 能将其编译为 Runtime Capability；Agent 能基于 World Model
> 做出 Decision；Decision 产生的 Effect 能**真实改变** World；World Model 能重新投影
> 变化后的世界，并把语义子图重新提供给 Agent。

## 1. 三层正式语义分离

```
① ONTOLOGY：世界是什么        Object / Relation / Capability / Policy / Action
② RUNTIME：世界怎么变化      Decision / Effect / Executor / Compensation / Event
③ WORLD MODEL：Agent 怎么认识世界  Entity / Relation / State / Subgraph / AgentView
```

完整闭环：`Ontology → Capability → Agent → Decision → Effect → World → Facts → World Model → Agent`

## 2. 决策定稿

| # | 决策 | 结论 |
|---|------|------|
| D1 | ODL → V2：IR → **RuntimeDefinition** → Service/Capability/Policy | ✅ |
| D2 | Entity：Relations 推导 + `EntityDataProvider` trait（V1 桥接留口，不依赖 V1 storage） | ✅ |
| D3 | GraphRAG = 图遍历 Subgraph（**Embedding 明确延后**，写入 Non-goals） | ✅ |
| D4 | **EffectExecutor 与 CompensationExecutor 分离**（评审修改后通过） | ⚠️→✅ |
| D5 | observe：裸 relations → World Model → **AgentView**（LLM 只看到 AgentView） | ✅ |

### D1 — RuntimeDefinition（Ontology Compiler → Runtime 的 ABI）

```
ODL / JSON Ontology / GraphQL Ontology / External / LLM-generated
        ↓
   RuntimeDefinition      ← 中间对象（不是 IR 直译成 Runtime 行为）
        ↓
   Service / Capability / Policy
```

```rust
pub struct RuntimeDefinition {
    pub object_type: String,
    pub capabilities: Vec<CapabilityDefinition>,
    pub policies: Vec<PolicyDefinition>,
    pub relations: Vec<RelationDefinition>,
}
```

V1 的 Ontology Semantic Asset 被**编译**为 V2 Runtime 能理解的语义，
而不是把 V1 ActionEngine 直接接进来（V2 拒绝成为 V1 的 wrapper）。

### D4 — 执行器分离（正向变化 ≠ 错误恢复）

```rust
pub trait EffectExecutor: Send + Sync {
    fn execute(&self, effect: &Effect) -> Result<EffectResult, EffectError>;
}

pub trait CompensationExecutor: Send + Sync {
    fn compensate(&self, compensation: &Compensation) -> Result<CompensationResult, CompensationError>;
}
```

- **Effect** 是正向世界变化；**Compensation** 是错误恢复语义——两者都改 World，语义不同，职责必须分开（避免 Executor 知道太多 Compensation）
- Phase 5 MVP：`RelationEffectExecutor` + `RelationCompensationExecutor`
- 未来：V1ActionEngineAdapter / DatabaseEffectExecutor / HttpEffectExecutor / WorkflowEffectExecutor

### Effect ≠ Projection（本阶段最重要的技术修复）

Phase 2/3 的 `relate(projection)` 是 MVP 模拟（假装世界已改变）。
Phase 5 升级为：

```
Decision → Effect → Executor → World Mutation（真实改变）
                         │
                         └→ Event/Audit → World Model 重新投影
```

Projection 描述**改变之后**的 World，不再代替改变。

## 3. World Model（Agent 认识世界的接口）

```rust
pub struct EntityView {
    pub id: String,
    pub entity_type: String,          // 前缀推导或 EntityDataProvider
    pub state: Value,
    pub outgoing: Vec<(String, String)>,  // (predicate, target)
    pub incoming: Vec<(String, String)>,  // (source, predicate)
}

pub struct Subgraph {
    pub seed: String,
    pub entities: Vec<EntityView>,
    pub relations: Vec<(String, String, String)>,
}

pub struct AgentViewV2 {              // observe 的正式产出（未来可扩）
    pub entities: Vec<EntityView>,
    pub relations: Vec<(String, String, String)>,
    pub states: Vec<(String, Value)>,
    pub subgraph: Option<Subgraph>,
    pub memory: Vec<MemoryItem>,
    pub capabilities: Vec<String>,
}
```

- Entity 由 V2 fact graph（relations 端点聚合）推导，**不依赖 V1 storage**
- `trait EntityDataProvider { fn get(&self, id: &str) -> Option<EntityData>; }` 默认 Null，
  V1 桥接（wb_ontology_object）后续实现
- Agent（尤其未来 LLM）**只能看到 AgentView**，不直接访问 World Storage

## 4. 模块结构

```
src/ontology/                 # 语义集成（D1）
├── mod.rs                    # RuntimeDefinition + ir_to_runtime 映射
├── adapter.rs                # IR → RuntimeDefinition（ODL actions/policy/link 映射）
└── runtime_def.rs            # RuntimeDefinition 类型（ABI）

src/world/                    # World Model 投影（D2/D3/D5）
├── mod.rs                    # World（已有 as_of/diff/replay）+ entity/subgraph 查询
├── entity.rs                 # EntityView 推导（relations 端点聚合）+ EntityDataProvider
├── subgraph.rs               # 图遍历检索（seed 可达）
└── view.rs                   # AgentViewV2 构建（observe 升级）

src/runtime_exec/             # 执行器分离（D4）
├── mod.rs
├── effect.rs                 # EffectExecutor trait + RelationEffectExecutor（V2-native）
└── compensation.rs           # CompensationExecutor trait + RelationCompensationExecutor
```

## 5. Non-goals（写入边界）

- ❌ Embedding / Vector DB / 语义相似度检索（D3 明确延后）
- ❌ V1 ActionEngine 适配器（D4 只做 V2-native RelationExecutor；V1 适配后续）
- ❌ V1 对象表桥接（EntityDataProvider 留口，V1 实现后续）
- ❌ 多 Ontology 源（RuntimeDefinition 保证 ABI 未来可接，本阶段只接 ODL）

## 6. 测试契约（tests/ontology_v2.rs）

| # | 场景 | 断言 |
|---|------|------|
| O1 | ODL(crm) → RuntimeDefinition → Service | capabilities/policies 数量与 ODL actions 一致 |
| O2 | Effect 真实改变 World（RelationExecutor） | emit+execute 后 relations 表有行；不再是"投影假装" |
| O3 | 补偿经 CompensationExecutor | 失败 → CompensationEngine → RelationCompensationExecutor 撤销 |
| O4 | EntityView 推导 | 关系图端点聚合出 entity + 出入边 |
| O5 | Subgraph 遍历 | seed 可达子图（1 跳/2 跳） |
| O6 | AgentViewV2 注入 | observe 返回 entities/subgraph，CognitiveEngine 基于语义视图决策 |
| O7 | ODL 闭环端到端 | ODL → Capability → Agent 决策 → Effect → 真实 World 变化 → World Model 重投影 → 下一轮 Observe 看到变化 |

## 7. 边界保持

- V1（run_goal / ActionEngine / storage / 155+ 测试）全部保留
- Kernel 五动作不变；TaskExecutor.step 挂接 EffectExecutor（Allow 后真实执行）
- AgentLoop.observe 升级为 AgentViewV2（对既有 A1-A7 测试保持兼容：仍提供 relations 视图）
