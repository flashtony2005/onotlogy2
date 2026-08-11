# 全量业务需求
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-02-001 |
| 所属模块 | WorkBuddy-Requirement |
| 版本 | V1.9 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
汇总 OpenAIP-OS 的业务需求，作为 PRD 拆分与验收标准的源头。**MVP = V0.3**（Agent 闭环）；V0.1/V0.2 是内核与世界模型的最小证明。

## 2. 业务/技术背景
- 向上依赖 `01_项目总览/project-overview.md` 的 MVP 战略（极简内核分形生长）。
- 向下是 `prd-template.md` 与各版本 `sprint-plan.md` 的输入。
- 被排除的能力见 `09_迭代与版本/todo-backlog.md`。

## 3. 核心规则 & 约束（重点）
- [强制] 每条需求必须有唯一编号、提出人、优先级、验收归属。
- [强制] 需求与代码、测试用例必须可追溯（需求编号贯穿 PRD/用例/代码注释）。
- [强制] MVP（=V0.3）核心模型仅含 **Object / Relation / Event** 三要素 + Agent 接入（Pi 作 Cognitive Engine）+ 自有 Planner；禁止在 MVP 引入 GraphRAG / 完整 Policy / Evolution 全套。
- [建议] 需求按 MoSCoW 法标注 Must/Should/Could/Won't。

## 4. 详细设计/内容

### 4.1 功能列表（按版本）
> V0.1 让对象有生命，V0.2 让世界成型，V0.3=MVP 让 Agent 住进世界并被人理解。

| 功能 | 模块 | 优先级 | 版本 |
|------|------|--------|------|
| 定义 Object（含 property） | ODL | Must | V0.1 |
| `.odl → IR` | Compiler | Must | V0.1 |
| 生成 Rust struct / schema.sql / ontology.json | Codegen | Must | V0.1 |
| ObjectEngine CRUD/query | Runtime | Must | V0.1 |
| Turso 持久化（LibSQLStorage） | Storage | Must | V0.1 |
| CLI compile / run / object list / event list | CLI | Must | V0.1 |
| Event Bus（RuntimeEvent 发布订阅） | Trace | Must | V0.1 |
| TraceEvent(kind=world) + `wb_trace_event` 存储 | Trace | Must | V0.1 |
| Relation `wb_relation_edge` 连接对象 | World Model | Should | V0.2 |
| 领域 Event（PaymentLate 等） | World Model | Should | V0.2 |
| 三层观察 UI + WebSocket 流 | Trace | Must | V0.3 |
| Agent 接入（Pi 作 Cognitive Engine，AgentEngine trait） | Agent | Must | V0.3 |
| Planner（OpenAIP 自有核心） | Agent | Must | V0.3 |
| 最小 Action 执行（Planner 闭环） | Agent | Must | V0.3 |

### 4.2 需求总表
| 需求编号 | 需求名称 | 提出人 | 优先级 | 状态 | 关联版本 |
|----------|----------|--------|--------|------|----------|
| REQ-001 | ODL 定义 Object 类型 | lushaohui | Must | 已确认 | V0.1 |
| REQ-002 | Compiler 将 .odl 编译为 IR | lushaohui | Must | 已确认 | V0.1 |
| REQ-003 | Codegen 生成 Rust struct / schema.sql / ontology.json | lushaohui | Must | 已确认 | V0.1 |
| REQ-004 | Runtime 提供 ObjectEngine CRUD/query | lushaohui | Must | 已确认 | V0.1 |
| REQ-005 | Storage 基于 Turso 持久化 | lushaohui | Must | 已确认 | V0.1 |
| REQ-006 | CLI 支持 compile / run / object list / event list | lushaohui | Must | 已确认 | V0.1 |
| REQ-007 | TraceEvent 模型 + `wb_trace_event` 存储（V0.1 承载 world 生命周期事件） | lushaohui | Must | 已确认 | V0.1 |
| REQ-008 | Event Bus（RuntimeEvent 发布订阅） | lushaohui | Must | 已确认 | V0.1 |
| REQ-009 | 三层观察界面（Executive/Operator/Developer）+ WebSocket 流 | lushaohui | Must | 已实现 | V0.3 |
| REQ-010 | Agent 接入（Pi 作 Cognitive Engine，AgentEngine trait）：NLU/推理；OpenAIP 拥有 Planner/Policy/Trace | lushaohui | Must | 已实现 | V0.3 |
| REQ-011 | Planner（OpenAIP 自有核心）：Goal→LLMGoalParser→Planner→PlanStep→Capability→Execute | lushaohui | Must | 已实现 | V0.3 |
| REQ-012 | Relation 引擎：建边/图查询（neighbors/query_by_edge）/重复边拦截(WB2007) | lushaohui | Must | 已实现 | V0.2 |
| REQ-013 | 领域 Event：PaymentLate 等业务事件经 `TraceEvent(kind=world)` 落库 + EventBus 发布 | lushaohui | Must | 已实现 | V0.2 |

### 4.3 核心模型需求
- Object / Relation / Event 为世界模型三要素：Object（V0.1）、Relation 与领域 Event（V0.2）成型，MVP（V0.3）齐备。
- Event 统一为 `TraceEvent`，`kind ∈ {world, cognitive}`：`world`（对象/关系生命周期，V0.1）、`cognitive`（Agent 认知，V0.3）；统一存 `wb_trace_event` 表。
- Action 分两层：最小执行（V0.3，保证 Planner 闭环）、完整引擎（V0.4）；Policy（V0.4）、Memory / Agent 自治（V0.5）后生长（见 todo-backlog.md）。
- 对象存储统一为 `wb_ontology_object { id, type, data, version }`，禁止为每类业务建独立表。

### 4.4 端到端示例（验收参照）
ODL 属性语法以 `03_架构设计/architecture-total.md` 4.2.1 为准：`properties:` 块 + 逐行 `name: type`，禁止 `property:` 单条形式。

**V0.1**：输入 `customer.odl`：

```odl
object Customer:
  properties:
    name: string
    level: string
```

编译、启动、查询、看事件：

```shell
oaip compile customer.odl      # 生成 generated/Customer.rs  schema.sql  ontology.json
oaip run
oaip object list Customer      # 001 Apple VIP / 002 Tesla GOLD
oaip event list                # world 事件：Customer_001 created ...
```

**V0.3（MVP）**：接入 Pi（作 Cognitive Engine）后，Agent 基于世界模型决策并经观察层暴露认知过程：

```shell
oaip agent run "reduce churn"
# Agent 查询 Customer → 经 Observation UI 暴露推理证据
# 输出：Goal → OntologyQuery → Reasoning → Planning → Action
```

## 5. 风险点 & 注意事项
- 需求膨胀需及时回滚到 `project-overview.md` 的 MVP 边界做取舍。
- Codegen 产物需与 `ontology-ir` 定义严格一致，避免文档/代码漂移。

## 6. 关联文档
- [../01_项目总览/project-overview.md](../01_项目总览/project-overview.md)
- [../02_需求设计/prd-template.md](../02_需求设计/prd-template.md)
- [../02_需求设计/acceptance-criteria.md](../02_需求设计/acceptance-criteria.md)
- [../03_架构设计/agent-observation-design.md](../03_架构设计/agent-observation-design.md)
- [../09_迭代与版本/sprint-plan.md](../09_迭代与版本/sprint-plan.md)
- [../09_迭代与版本/todo-backlog.md](../09_迭代与版本/todo-backlog.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成 |
| V1.1 | lushaohui | 2026-08-11 | 填入 MVP V0.1 功能列表、需求总表、端到端示例 |
| V1.2 | lushaohui | 2026-08-11 | 新增 Agent Observation Kernel 相关需求（REQ-007/008/009） |
| V1.3 | lushaohui | 2026-08-11 | 纠正：MVP 包含 Agent 接入；新增 REQ-010（PI Agent，LLM 驱动）；移除"禁止 Agent"约束 |
| V1.4 | lushaohui | 2026-08-11 | Pi 降级为 Cognitive Engine（AgentEngine）；新增 REQ-011 Planner（OpenAIP 自有核心） |
| V1.5 | lushaohui | 2026-08-11 | 按方案B重排版本归属：MVP=V0.3；REQ-007/008 归 V0.1（观察基建），REQ-009/010/011 归 V0.3；Event 统一为 TraceEvent(world/cognitive)；Action 分两层 |
| V1.6 | lushaohui | 2026-08-11 | 表名对齐 table-design 约束：`trace_event` → `wb_trace_event` |
| V1.7 | lushaohui | 2026-08-11 | ODL 属性语法定版：以 architecture-total 4.2.1 为准（`properties:` 块 + `name: type`） |
| V1.8 | lushaohui | 2026-08-11 | V0.2 落地：补 REQ-012(Relation 引擎)/REQ-013(领域 Event)，状态已实现 |
| V1.9 | lushaohui | 2026-08-12 | V0.3=MVP 落地：REQ-009/010/011 状态置「已实现」（三层观察 UI + WebSocket + Pi 作 Cognitive Engine + 自有 Planner 闭环，TC-009/010/011 全绿） |
