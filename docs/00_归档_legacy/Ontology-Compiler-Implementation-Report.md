> [废弃] 历史归档，已被现行文档体系（docs/01~10）取代，仅作溯源参考，不作为实现依据。

# OpenAIP-OS Ontology Compiler 实现报告

> 版本：V1.3 实现报告（Implementation Report）
> 配套文档：OpenAIP-OS V1.2（ODL + Runtime Kernel）、V1.3（Compiler + Rust Runtime）、Palantir-OSS-Reference.md（成熟方案基准）
> 设计原则：遵循项目根 `AGENTS.md` 的 8 条——不保留向后兼容、最简单实现、分层构建、模块化、成熟依赖优先、先查已有依赖、长期决策、参考成熟方案。
> 参考基准：Palantir `osdk-ts`（字符串模板 codegen + metadata-as-const）、`conjure`（YAML→IR→多语言生成 + 兼容闸门）。

---

## 0. 范围与产出

本报告回答一个具体问题：**Ontology Compiler 的 Code Generator 与 Runtime 绑定怎么实现**，才能既让调用方获得"编译期类型安全"（像 Palantir OSDK 的 `objects.Employee.get()`），又让 Runtime Kernel 用一套泛型引擎驱动任意本体对象（"可执行世界模型"）。

范围：
- ✅ OntologyIR 规格（语言无关中间表示）
- ✅ 类型映射表（wireType → Rust，索引表驱动）
- ✅ 代码生成器设计（字符串模板，对齐 OSDK）
- ✅ 生成产物结构（struct + `const METADATA` + trait 约束）
- ✅ Runtime 绑定（泛型内核如何消费 METADATA）
- ✅ 向后兼容闸门（支撑 V1.6 自我进化）
- ❌ Lexer/Parser/AST 细节（V1.3 §4–6 已定义，本报告假定其产出 AST）
- ❌ 前端/UI Schema 生成（仅列 target，不展开）

---

## 1. 背景与缺口

V1.2/V1.3 已定义完整链路：`.odl → AST → OntologyIR → 多 target 生成 → Runtime Kernel`。但 V1.3 §9 仅给了生成结果的"外形"（如 `#[derive(Object)]`、Turso `CREATE TABLE`），**未定义**：
1. IR 的精确数据结构（字段、关系、动作、策略如何统一表达）。
2. wireType → 目标语言类型的映射机制。
3. 生成物与 Runtime Kernel 之间的**耦合点**：Kernel 是泛型引擎，但生成物是具体类型，二者如何衔接而不退化成"每个对象写一遍"。

本报告基于 Palantir 开源实现补齐这三点。核心借鉴：

| Palantir 模式 | 解决我们的问题 |
|---|---|
| OSDK：生成 `const X` 携带元数据 + 命名空间类型 | 让"编译期类型"与"运行期可反射元数据"共存 |
| OSDK：字段映射走 `$PropType[wireType]` 索引表 | 类型映射零分支、可扩展 |
| OSDK：codegen 用字符串模板 + `stringify` | 简单优先，不引 AST 框架 |
| Conjure：YAML → IR → 多语言生成 | 我们的 IR 必须是语言无关的单一真相源 |
| Conjure：`conjure-backcompat` 校验 IR | 本体演化需要 IR 级兼容闸门 |

---

## 2. 总体架构

```
.odl (ODL 源)
   │  Lexer / Parser / Semantic (V1.3 §4–7)
   ▼
OntologyAST                      （V1.3 §6，已定义）
   │  Lowering（AST → IR）
   ▼
OntologyIR  ──────────────────┐ （语言无关，单一真相源）
   │                          │
   │  Backward-Compat Gate    │ （IR 级 schema 校验，支撑 V1.6）
   ▼                          │
Code Generator (string templates)
   ├── Rust Object Models  ───┤→  crate::generated::{type + const METADATA}
   ├── Turso Schema         ──┤→ schema.sql
   ├── Agent Tool JSON      ──┤→ agent_tools.json (MCP)
   ├── GraphRAG Mapping     ──┤→ ontology_graph.json
   └── (future) TS SDK      ──┘→ 对齐 OSDK 范式
   ▼
Runtime Kernel（泛型引擎，消费 `&ObjectDef`）
   ├── ObjectEngine / RelationEngine / ActionEngine / PolicyEngine / EventEngine
```

设计要点（对应 AGENTS.md）：
- **IR 是唯一真相源**（#8 参考成熟方案：Conjure IR）。新增 target（TS/Go/Python）只需加一个 generator，不动 IR。
- **生成与运行时解耦**（#4 模块化）：codegen 只产出"类型 + 元数据"，Runtime 只读元数据驱动。
- **分层构建**（#3）：先跑通 Rust target + Turso + ObjectEngine 最小闭环，再加 Agent Tool / GraphRAG / 兼容闸门。

---

## 3. OntologyIR 规格

IR 必须是**语言无关**的。每个字段类型用 `WireType` 枚举表达（对齐 OSDK 的 `PropertyValueWireToClient` 索引键），而非直接写 Rust 类型。

```rust
// ontology-ir/src/lib.rs
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OntologyIR {
    pub name: String,
    pub version: String,
    pub objects: Vec<ObjectType>,
    pub relations: Vec<RelationType>,   // 独立边类型（可选，V1.2 §7）
    pub actions: Vec<ActionType>,
    pub policies: Vec<PolicyDef>,
    pub events: Vec<EventDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObjectType {
    pub api_name: String,
    pub description: Option<String>,
    pub primary_key: String,
    pub properties: Vec<PropertyDef>,
    pub relations: Vec<RelationDef>,     // 指向其他 Object 的边
    pub actions: Vec<String>,            // 引用的 ActionType.api_name
}

// —— 字段类型用 WireType 表达，不直接绑定语言 ——
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WireType {
    String, Integer, Float, Boolean,
    Datetime,            // RFC3339
    Attachment,          // 文件引用
    Enum { values: Vec<String> },
    Struct { fields: Vec<PropertyDef> },
    ObjectRef(String),   // 指向某个 ObjectType
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PropertyDef {
    pub api_name: String,
    pub wire_type: WireType,
    pub multiplicity: bool,  // true → Vec<T>
    pub nullable: bool,      // true → Option<T>
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RelationDef {
    pub api_name: String,
    pub target: String,            // ObjectType.api_name
    pub cardinality: Cardinality,  // One / OneToMany / ManyToMany
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Cardinality { One, OneToMany, ManyToMany }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActionType {
    pub api_name: String,
    pub input: Vec<PropertyDef>,        // 入参
    pub permission: Vec<String>,        // 引用的角色/策略
    pub executor: Option<String>,       // 如 "OrderService.approve"
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicyDef {
    pub api_name: String,
    pub when_action: String,
    pub condition: Option<String>,      // 表达式，V1.2 §10
    pub require_role: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventDef {
    pub api_name: String,
    pub object: String,
    pub payload: Vec<PropertyDef>,
}
```

**为什么 WireType 用枚举而非字符串别名**：序列化到 JSON IR 后，Conjure 式兼容闸门可对 `WireType` 做结构化 diff（见 §7），比字符串比较可靠。

---

## 4. 类型映射表（索引表驱动，零分支）

对齐 OSDK：`$PropType[JSON.stringify(propertyDefinition.type)]`。我们建一张 `WireType → Rust 类型片段` 的查找表，**不用 if/else**（#2 最简单、#7 长期）。

```rust
// codegen-rust/src/type_map.rs
use ontology_ir::WireType;

/// 返回 Rust 类型 token（不含 multiplicity/nullable 包装）
pub fn rust_scalar(wt: &WireType) -> String {
    match wt {
        WireType::String    => "String".into(),
        WireType::Integer   => "i64".into(),
        WireType::Float     => "f64".into(),
        WireType::Boolean   => "bool".into(),
        WireType::Datetime  => "chrono::DateTime<Utc>".into(),
        WireType::Attachment=> "crate::Attachment".into(),
        WireType::Enum { .. } => "String".into(),        // 生成同名 enum，此处占位
        WireType::Struct { .. } => "serde_json::Value".into(), // 或生成内联 struct
        WireType::ObjectRef(name) => name.clone(),
    }
}

/// 套上 multiplicity / nullable 包装
pub fn rust_type(prop: &PropertyDef) -> String {
    let base = rust_scalar(&prop.wire_type);
    let with_multi = if prop.multiplicity { format!("Vec<{base}>") } else { base };
    if prop.nullable { format!("Option<{with_multi}>") } else { with_multi }
}
```

映射表即扩展点：未来加 `WireType::GeoPoint`，只在 `rust_scalar` 加一行 + 在 TS generator 加一行，IR 与其他 target 不受影响。**这正是 IR 单一真相源的价值**。

---

## 5. 代码生成器设计（字符串模板，对齐 OSDK）

**技术选型**：字符串模板 + 一个 `render()` 小工具，**不引入 ts-morph / syn 代码构建框架**（#2、#5）。OSDK 实测即用 `stringify()` + `formatTs()`，足够且更简单。我们同理：`format_rust()` 用 `prettyplease` 或 `rustfmt` 美化，但生成阶段只拼字符串。

```rust
// codegen-rust/src/object_gen.rs
use ontology_ir::{ObjectType, OntologyIR};
use crate::type_map::rust_type;

pub fn gen_object(obj: &ObjectType) -> String {
    let props_struct: String = obj.properties.iter().map(|p| format!(
        "    pub {name}: {ty},", name = p.api_name, ty = rust_type(p)
    )).collect();

    let props_meta: String = obj.properties.iter().map(|p| format!(
        "        PropertyDef {{ api_name: \"{n}\", wire_type: {wt:?}, multiplicity: {m}, nullable: {nu} }},",
        n = p.api_name, wt = p.wire_type, m = p.multiplicity, nu = p.nullable
    )).collect();

    let rels_meta: String = obj.relations.iter().map(|r| format!(
        "        RelationDef {{ api_name: \"{n}\", target: \"{t}\".into(), cardinality: Cardinality::{c:?} }},",
        n = r.api_name, t = r.target, c = r.cardinality
    )).collect();

    format!(r#"
// @generated by open-aip compiler — DO NOT EDIT
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, OntologyObject)]
pub struct {name} {{
{props_struct}
}}

pub const {NAME}_DEF: ObjectDef = ObjectDef {{
    api_name: "{name}",
    primary_key: "{pk}",
    properties: &[
{props_meta}
    ],
    relations: &[
{rels_meta}
    ],
    actions: &[],
}};

impl OntologyObject for {name} {{
    fn object_def() -> &'static ObjectDef {{ &{NAME}_DEF }}
}}
"#, name = obj.api_name, NAME = obj.api_name.to_uppercase(), pk = obj.primary_key)
}
```

要点：
- 生成物是 `struct` + `const METADATA` + trait impl 三件套（对齐 OSDK 的 namespace/interface/const 三件套）。
- `METADATA`（`ObjectDef`）是**运行期可反射的元数据**；`struct` 是**编译期类型安全表面**。
- `#[derive(OntologyObject)]` 提供默认方法，`object_def()` 返回 `&'static ObjectDef` —— 让泛型 Kernel 拿到元数据。

---

## 6. 生成产物总览（多 target）

| Target | 产物 | 作用 | 来源 |
|---|---|---|---|
| Rust 对象模型 | `generated/{Name}.rs` | 编译期类型安全 + METADATA | OSDK 范式（本报告 §5） |
| Turso Schema | `schema.sql` | `object_{x}` 表 + `relation_edge` + `event_log` | V1.2 §12 / V1.3 §9.1 |
| Agent Tool JSON | `agent_tools.json` | MCP Tool Contract（本体即 Tool） | V1.2 §13 |
| GraphRAG Mapping | `ontology_graph.json` | 实体/关系抽取锚点 | V1.3 §12 |
| (未来) TS SDK | `sdk.ts` | 前端消费本体对象 | 对齐 OSDK / Conjure |

Turso 生成沿用 V1.2 §12 既定形态（`data JSON` + 单表宽写），本报告不重定义。Agent Tool 沿用 V1.2 §13 形态，但**从 ActionType IR 直接生成**（不再手写），保证契约与本体同源。

---

## 7. 运行时绑定（泛型 Kernel 消费 METADATA）

这是"可执行世界模型"的关键：ObjectEngine 不认识具体类型，只认识 `&ObjectDef` 与 `serde_json::Value`。具体类型只在**调用方边界**提供类型安全。

```rust
// ontology-runtime/src/object_engine.rs
use ontology_ir::ObjectDef;
use ontology_core::OntologyObject;

pub trait ObjectEngine {
    async fn create(&self, def: &ObjectDef, data: serde_json::Value) -> Result<String>;
    async fn get(&self, def: &ObjectDef, id: &str) -> Result<serde_json::Value>;
    async fn update(&self, def: &ObjectDef, id: &str, patch: serde_json::Value) -> Result<()>;
    async fn query(&self, def: &ObjectDef, filter: Query) -> Result<Vec<serde_json::Value>>;
}

// 调用方边界：类型安全包装（对齐 OSDK client(Employee).where(...)）
pub async fn get_typed<E: ObjectEngine, T: OntologyObject>(
    engine: &E, id: &str,
) -> Result<T> {
    let raw = engine.get(&T::object_def(), id).await?;
    Ok(serde_json::from_value(raw)?)   // 编译期 T 保证类型安全
}

// Agent / Planner 路径：只持 &ObjectDef，动态驱动
//   action "Customer.updateDiscount" → 查 CUSTOMER_DEF → Policy 校验 → ObjectEngine.update
```

**耦合点只有一个**：`OntologyObject::object_def() -> &'static ObjectDef`。
- 生成物提供 `object_def()`（§5）。
- 泛型 Kernel 只依赖 `ObjectDef`，不依赖具体 struct。
- 调用方用 `get_typed::<Customer>()` 获得类型安全；Agent 用 `get(&CUSTOMER_DEF, ...)` 动态驱动。

这正是 OSDK 的本质：**元数据即值，类型与元数据并存**。区别是 OpenAIP-OS 用 Rust `const` + trait，OSDK 用 TS `const` + `satisfies`。

---

## 8. 向后兼容闸门（支撑 V1.6 自我进化）

对齐 Conjure `conjure-backcompat`：对 **IR 的 JSON Schema** 做兼容性校验，而非对生成代码。

规则（最小化、可机械化校验）：
1. **不可删除**已发布的 `ObjectType.api_name` / `PropertyDef.api_name`。
2. **不可改** `primary_key`。
3. **不可收窄**类型：`String → Integer` 非法；`Option<T> → T`、`Vec<T> → T` 非法。
4. **可放宽**：`T → Option<T>`、`T → Vec<T>`、`Integer → String` 合法。
5. **新增** Object/Property/Action 永远合法。

实现：用 `schemars` 为 `OntologyIR` 派生 JSON Schema，旧版本 schema 存 `ir.schema.json`，新 IR 生成新 schema 后跑相容性 diff（`opendelta` 或自写规则校验）。CI 中默认开启；V1.6 自我进化模块在改写本体前必须先过此闸门。

---

## 9. 实施路线（分层，#3）

**MVP（最小可跑通闭环）**
1. `ontology-ir`：定义 §3 结构体 + `schemars` derive。
2. AST → IR lowering（复用 V1.3 §6 AST）。
3. `codegen-rust`：仅 Rust 对象模型 + Turso schema（§4–5）。
4. `ontology-runtime`：`ObjectEngine` + `TursoStorage`（V1.3 §10–11）+ `get_typed` 边界（§7）。
5. `oaip compile crm.odl` CLI（V1.3 §15）跑通：`.odl → ir → rust + sql`。
6. 端到端：编译 → 启动 → Agent 通过 `get_typed`/动态 `ObjectDef` 读写。

**第二阶段（执行与治理）**
7. `ActionEngine` + `PolicyEngine` 消费 `ActionType`/`PolicyDef` IR。
8. Agent Tool JSON + MCP 暴露（V1.2 §13 / V1.3 §13）。

**第三阶段（进化与扩展）**
9. 向后兼容闸门（§8）。
10. GraphRAG mapping 生成（V1.3 §12）。
11. (未来) TS SDK generator，对齐 OSDK 范式，前端消费本体对象。

---

## 10. 依赖与工具（#5 成熟依赖优先）

| 用途 | 选型 | 理由 |
|---|---|---|
| IR 序列化 | `serde` + `schemars` | 生态标准；`schemars` 直接出 JSON Schema 供兼容闸门 |
| Rust 美化 | `prettyplease` 或 `rustfmt` | 生成代码可读性，不自己写 formatter |
| 时间 | `chrono` | OSDK 用 datetime，标准方案 |
| 模板 | 原生 `format!` / 小 `render!` 宏 | 不引 AST 框架（对齐 OSDK） |
| CLI | `clap` | V1.3 §15 `oaip` 子命令 |
| 存储 | `libsql` / `rusqlite` | 已有选型（turso 迁移中） |

---

## 11. 风险与权衡

| 风险 | 缓解 |
|---|---|
| 字符串模板生成易出错（缺分号/缩进） | 生成后跑 `rustfmt`；CI 对生成物 `cargo check` |
| `serde_json::Value` 动态路径丢失类型安全 | 仅在 Agent/Planner 边界用；用户代码走 `get_typed<T>` |
| IR 膨胀（五维全塞进 IR） | IR 只表达"结构"，执行语义（executor 实现）留在外部 service，IR 仅存引用字符串 |
| 兼容闸门规则过严阻碍进化 | 规则机械可判定（§8），不靠人工评审 |

---

## 12. 结论

本报告的实质贡献是把 V1.3 的"生成产物外形"升级为**可落地的耦合设计**：
1. **OntologyIR 用 `WireType` 枚举**做语言无关单一真相源。
2. **类型映射用索引表**（零分支），扩展点清晰。
3. **生成物 = struct + `const METADATA` + trait impl**，让"编译期类型"与"运行期反射元数据"共存（OSDK metadata-as-const）。
4. **Runtime Kernel 只依赖 `&ObjectDef`**，通过 `OntologyObject::object_def()` 单一耦合点驱动任意对象。
5. **IR 级向后兼容闸门**支撑 V1.6 自我进化。

下一步建议从 §9 MVP 第 1–6 步落地，先跑通「`.odl → Rust 对象 + Turso → 读写`」最小闭环，再叠加执行与治理。

> 基准来源：github.com/palantir/osdk-ts（packages/generator/src/v2.0）、github.com/palantir/conjure（2026-08-10 抓取）。
