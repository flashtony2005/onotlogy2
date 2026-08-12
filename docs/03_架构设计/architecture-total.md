# 整体架构
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-03-001 |
| 所属模块 | WorkBuddy-Architecture |
| 版本 | V1.8 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
定义 OpenAIP-OS MVP 的整体架构闭环、最小 Ontology Kernel 设计、保持内核极简的三原则，以及 MVP 明确排除的能力，作为后续模块拆分（module-split.md）与演进路线（sprint-plan.md）的总纲。

## 2. 业务/技术背景
- 来源：`01_项目总览/project-overview.md`（定位与哲学）、`00_归档_legacy/OpenAIP-OS-V1.3.md`（Compiler 路线）、`00_归档_legacy/Palantir-OSS-Reference.md`（IR / codegen-runtime 分离借鉴）。
- 技术基调：Rust + Turso(libSQL)，MVP 单体，不拆微服务。

## 3. 核心规则 & 约束（重点）
- [强制] 内核架构闭环（V0.1）固定为：`.odl → Ontology Compiler → Ontology IR → Runtime Kernel → Turso → Object CRUD`；MVP（V0.3）在其上叠加 Agent 闭环。
- [强制] Kernel 不知道业务：禁止 `CustomerEngine` / `OrderEngine` / `RiskEngine`，只允许 `ObjectEngine` / `RelationEngine` / `EventEngine`。
- [强制] Metadata 驱动一切：运行时由 `ObjectDef` 驱动，禁止 `match object_type { "Customer" => .. }` 硬编码分支。
- [强制] 所有高级能力（GraphRAG / Policy / Workflow / Evolution）必须作为插件挂到 Kernel 之上，Kernel 本身长期稳定。
- [强制] 可观察性与内核同生：V0.1 落地 Event Bus + TraceEvent(world) + CLI 观察，V0.3 升级为三层图形 UI + CognitiveEvent。
- [强制] V0.3（=MVP）**接入 Agent**：以 Pi（`@earendil-works/pi-coding-agent`）作 **Cognitive Engine**（经 `AgentEngine` trait 提供推理 / NLU / 工具调用）；OpenAIP 自身拥有 Agent Runtime（Planner / Policy / Trace）、Ontology Kernel 与 World Model，详见 `pi-agent-integration.md`；V0.5 落地 Memory 沉淀（Agent 自治基础）与 Evolution Engine MVP（REQ-017，Learn≠Permission：不修改 Policy、不绕过审批），自学习合成 / Multi-Agent 更远（见 todo-backlog）。

## 4. 详细设计/内容

### 4.1 内核架构闭环（V0.1）
```
        .odl
         ↓
   Ontology Compiler
         ↓
    Ontology IR
         ↓
  Runtime Kernel
         ↓
     Turso
         ↓
  Object CRUD
```
目标：让一个对象拥有生命（可定义、可创建、可查询）。这是 MVP（V0.3）的内核基础。

### 4.2 最小 Ontology Kernel 设计（Kernel v0.1）
Kernel 只含 5 个核心概念：

```
OntologyKernel
    │
    ├── ObjectDef        # 世界中的类型
    ├── ObjectInstance    # 现实对象
    ├── Relation          # 世界连接
    ├── Event             # 时间维度
    └── Storage           # 持久化
```

#### 4.2.1 ObjectDef（世界中的类型）
定义示例 `customer.odl`：

```odl
object Customer:
  properties:
    name: string
    level: string
```

- [强制] ODL 属性语法定版：属性容器统一为 `properties:` 块，属性逐行 `name: type`（标量类型：`string` / `int` / `float` / `bool`）；禁止使用 `property:` 单条形式。

Compiler 生成（符合 V1.3 生成物：struct + const METADATA + trait impl）：

```rust
pub struct Customer {
    name: String,
    level: String,
}

pub const CUSTOMER_DEF: ObjectDef = /* .. */;
```

#### 4.2.2 ObjectInstance（现实对象）
存储表 `wb_ontology_object`：

| 字段 | 含义 |
|------|------|
| id | 对象 ID |
| type | 类型（如 Customer） |
| data | 对象数据（JSON） |
| version | 版本号 |

示例数据：

```json
{
  "id": "customer_001",
  "type": "Customer",
  "data": { "name": "Apple", "level": "VIP" }
}
```

#### 4.2.3 Relation（世界连接）
MVP 仅需边表 `wb_relation_edge`：

| 字段 | 含义 |
|------|------|
| source | 源对象 ID |
| relation | 关系名（如 purchased） |
| target | 目标对象 ID |

示例：`Customer --purchased--> Order`。

#### 4.2.4 Event（时间维度）
数据库描述"现在是什么"，Event 描述"发生过什么"。Event 统一为 `TraceEvent`，`kind ∈ {world, cognitive}`：`world` 承载对象/关系生命周期（V0.1）与领域事件如 `PaymentLate → RiskIncrease`（V0.2），`cognitive` 承载 Agent 认知（V0.3）；统一存 `wb_trace_event` 表。

### 4.3 保持内核极简的三原则

**原则 1：Kernel 不知道业务**
错误：为每类业务写特化 Engine（`CustomerEngine`）。
正确：只有 `ObjectEngine` / `RelationEngine` / `EventEngine`，Kernel 永远不知道 `Customer` 是什么。

**原则 2：Metadata 驱动一切**
不要 `match object_type { "Customer" => {} }`，而应由 `ObjectDef` 流入 Runtime 驱动行为。这正是 Palantir OSDK 核心思想：**metadata-as-const**。

**原则 3：所有高级能力都是插件**
```
Runtime Kernel
       │
  ─────────────
  Agent Plugin
  GraphRAG Plugin
  Policy Plugin
  Workflow Plugin
  Evolution Plugin
```
Kernel 永远稳定，能力外挂。

### 4.4 能力边界（MVP = V0.3）
| 能力 | 引入版本 | 原因 |
|------|----------|------|
| ✅ 可观察基建（Event Bus + TraceEvent(world) + CLI 观察） | V0.1 | 可观察性与内核同生，为 Agent 观察打地基 |
| ✅ World Model（Relation + 领域 Event） | V0.2 | 三要素成型，Agent 的世界底座 |
| ✅ Agent 接入（Pi Cognitive Engine + Planner + 最小 Action + 观察 UI） | V0.3（=MVP） | Pi 经 `AgentEngine` 提供认知；OpenAIP 拥有 Planner / Policy / Ontology / World Model / Trace，详见 `pi-agent-integration.md` |
| ✅ 完整 Action Engine（+ ActionRegistry / ActionEngine，ODL `actions:` 声明式） | V0.4 | 数据驱动解释执行 `updates`/`creates`/`link`（幂等），替代硬编码计划；Policy 审批在 V0.4 落地 |
| ✅ Policy 审批约束（动作级 `policy:`：require_approval 闸门 + precondition 前置条件，拒 → WB2005） | V0.4 | 完整 Action 引擎 + Policy 均在 V0.4 落地；审批经 `PolicyApprover` 放行（CLI `ConsoleApprover` / `AutoApprover`），server 侧 `require_approval` 动作挂起并暴露 HTTP 审批端点（`ServerApprover`+`ApprovalStore`），浏览器点批准/拒绝后恢复或 WB2005 终止 |
| ✅ Memory 沉淀（Agent 自治基础，REQ-016） | V0.5 | 经验按 `action.<name>` upsert 沉淀 `wb_memory`（hits 累加），下一轮 run 经 `MemoryRecall` 认知事件召回，形成 Action→Memory→下次 Plan 闭环 |
| ✅ 观察 UI · Memory 视图 | V0.5 | 三层认知导航新增 Memory 标签，前端经 `GET /api/v1/wb/agent/memory` 渲染沉淀列表（label/payload/hits/updatedAt），运行前载入 + MemoryUpdate/MemoryRecall 事件实时刷新；TC-024a 全绿 |
| ❌ 完整 GraphRAG | 世界模型成熟后 | 是 Ontology + Knowledge Graph + Embedding 的上层；没有 Object Graph 则无意义 |
| ✅ Evolution Engine MVP（REQ-017 · Learn≠Permission） | V0.5 | Experience→Pattern→Proposal→Validation→LearnedKnowledge 闭环，可观察/可验证/可撤销，**不修改 Policy、不绕过审批**；`run_evolution` 幂等；`GET /api/v1/wb/agent/evolution` 暴露状态；AutoApprove 类「Knowledge→Permission」升级归第二阶段 |
| ❌ Multi-Agent | V3 | 属 Agent Society（Protocol/Consensus/Delegation） |

### 4.5 技术栈选型
| 维度 | 选型 | 理由 |
|------|------|------|
| 语言 | Rust | 安全、性能、与 V1.3 一致 |
| 存储 | Turso / libSQL | Local First、嵌入式、单行对象存储友好 |
| 形态 | MVP 单体（crates 内聚） | 不过度拆微服务，见 module-split.md |
| IR | 语言无关单一真相源 | 借鉴 Palantir，便于未来多语言 codegen |

### 4.6 Agent 可观察性架构（MVP 必须包含）
观察层分两步走：V0.1 先落 Event Bus + TraceEvent(world) + CLI 观察（与内核同生），V0.3 升级为 Human 在环的三层图形 UI（详设见 `agent-observation-design.md`）：

```
                Human
                  │
        Agent Observation UI
                  │
             Trace API
                  │
        Agent Runtime
       │               │
 Ontology Kernel     Event Bus
       │
     Turso
```

要点：
- Agent 每一步（Goal/Intent/Query/Reasoning/Planning/Policy/Action/Memory）必须绑定 Ontology 对象，UI 展示"Reasoning Evidence"而非裸日志。
- 事件经 Event Bus 分发至 Trace Storage / UI Stream / Memory / Audit，同源消费。
- V0.3（=MVP）接入 **Pi（`@earendil-works/pi-coding-agent`）作 Cognitive Engine** 承担推理，OpenAIP 的 Agent Runtime 负责决策编排，观察层暴露认知过程；Evolution / Multi-Agent 等高级自治延后（分层与 Pi 定位见 4.7）。

### 4.7 OpenAIP Agent Runtime 分层与 Pi 定位
OpenAIP-OS 的核心不是"Pi 增强版"，而是拥有 Agent Runtime 的操作系统；Pi 仅是其可替换的认知引擎：

```
Human → Agent UI → Decision Trace UI
        ↓
================================
     OpenAIP Agent Runtime
   [Planner | Policy | Ontology Runtime
    | World Model | Event Bus | Capability Registry]
        ↓
================================
        pi agent  (Cognitive Engine)
     LLM Agent Execution Engine
        ↓
================================
       Qwen / Claude / GPT
```

- OpenAIP 自有资产：**Ontology Kernel + World Model + Agent Runtime（Planner / Policy）+ Decision Trace**——这是 Palantir AIP 难复制的部分。
- Pi 仅提供认知：推理 / NLU / 工具调用；经 `AgentEngine` trait 接入，未来可替换为 Claude / Qwen / LocalLLM 引擎（见 `pi-agent-integration.md`、`module-split.md` 4.3）。
- V0.3（=MVP）执行流：用户输入 → Pi(NLU) → Intent → Ontology Query → Planner → Action → Executor → Trace UI。

## 5. 风险点 & 注意事项
- 架构图与代码实际不符会误导维护，变更须同步更新本文件与 module-split.md。
- 极简原则易被"临时特化"侵蚀，代码评审需卡原则 1/2。
- Relation/领域 Event 须严格按 V0.2 节奏引入，Agent/Planner 按 V0.3 节奏引入，避免提前污染内核。

## 6. 关联文档
- [../01_项目总览/project-overview.md](../01_项目总览/project-overview.md)
- [../03_架构设计/module-split.md](../03_架构设计/module-split.md)
- [../03_架构设计/agent-observation-design.md](../03_架构设计/agent-observation-design.md)
- [../03_架构设计/pi-agent-integration.md](../03_架构设计/pi-agent-integration.md)
- [../03_架构设计/security-design.md](../03_架构设计/security-design.md)
- [../09_迭代与版本/sprint-plan.md](../09_迭代与版本/sprint-plan.md)
- [../09_迭代与版本/todo-backlog.md](../09_迭代与版本/todo-backlog.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成 |
| V1.1 | lushaohui | 2026-08-11 | 填入 MVP 架构闭环、最小 Kernel 设计、极简三原则、排除项 |
| V1.2 | lushaohui | 2026-08-11 | 加入 Agent Observation Kernel（可观察性层）、调整架构图、细化排除项 |
| V1.3 | lushaohui | 2026-08-11 | 纠正：MVP 包含 Agent 接入（PI Agent，LLM 驱动）；重写 4.4 能力边界表，移除"禁止 Agent"误述 |
| V1.4 | lushaohui | 2026-08-11 | 新增 4.7 分层架构；Pi 降级为 Cognitive Engine（AgentEngine trait），OpenAIP 拥有 Agent Runtime |
| V1.5 | lushaohui | 2026-08-11 | 方案B对齐：4.1 定位内核闭环(V0.1)；Event 统一为 TraceEvent(world/cognitive)；4.4 能力边界按版本归属（MVP=V0.3）；可观察层分两步 |
| V1.6 | lushaohui | 2026-08-11 | 表名对齐 table-design 约束：`trace_event` → `wb_trace_event` |
| V1.7 | lushaohui | 2026-08-11 | ODL 属性语法定版：统一 `properties:` 块 + `name: type`，禁止 `property:` 单条形式 |
| V1.8 | lushaohui | 2026-08-11 | 明确 codegen 产物：生成 `pub const X_DEF: ObjectDef`（新增 ObjectDef 类型，内部持 &'static str JSON，编译期合法、运行时 serde 解析驱动，杜绝 match 硬编码），与 4.2.1 及原则2（metadata-as-const）一致 |
| V1.9 | lushaohui | 2026-08-12 | V0.3=MVP 能力边界标记已实现 |
| V2.0 | lushaohui | 2026-08-12 | V0.4 完整 Action 引擎落地：4.4 能力边界表「完整 Action Engine」由 ❌ 改 ✅（数据驱动 ActionRegistry/ActionEngine）；Policy 审批单列待办 |
| V2.1 | lushaohui | 2026-08-12 | V0.4 Policy 引擎落地：4.4 能力边界「Policy 审批约束」由 ❌ 改 ✅（动作级 policy: 审批闸门 + 前置条件，经 PolicyApprover 放行） |
| V2.2 | lushaohui | 2026-08-12 | 交互式 Human Approval 落地：4.4「Policy 审批约束」补 CLI `ConsoleApprover`（stdin 交互）+ server `--auto-approve` 开关；TC-021a/b 全绿 |
| V2.3 | lushaohui | 2026-08-12 | V0.5 Memory 沉淀落地：4.4 能力边界表新增「Memory 沉淀（REQ-016）」由 ❌ 改 ✅（Action→Memory→下次 Plan 闭环）；Evolution Engine 行改标「远期」；TC-022a/b 全绿 |
| V2.4 | lushaohui | 2026-08-12 | server 挂起式 HTTP 审批端点（V0.4 遗留项）落地：4.4「Policy 审批约束」补 `ServerApprover`+`ApprovalStore` 与 `GET /run/:run_id/approvals` + `POST /run/:run_id/approval/:req_id`（挂起→浏览器批准恢复 / 拒绝→WB2005 终止）；`PolicyApprover::approve` 升 async；TC-023a/b 全绿 |
| V2.5 | lushaohui | 2026-08-12 | 观察 UI 接入 Memory 视图：4.4 能力边界表新增「观察 UI · Memory 视图」由 ❌ 改 ✅；后端 `GET /api/v1/wb/agent/memory` + 前端 Memory 标签；TC-024a 全绿 |
| V2.6 | lushaohui | 2026-08-12 | Evolution Engine MVP（REQ-017）落地：4.4 能力边界表「Evolution Engine（自学习/自我进化）」由 ❌ 改 ✅（MVP，Learn≠Permission）；新增 `src/evolution.rs` + storage 三表 + RuntimeKernel `run_evolution` + server `GET /api/v1/wb/agent/evolution`；TC-025a~h 全绿 |
