# Agent 认知过程观察界面设计（Agent Cognition Trace Interface）
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-03-005 |
| 所属模块 | OpenAIP-OS-Architecture |
| 版本 | V1.5 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
定义 OpenAIP-OS 的 **Agent 认知过程观察界面（Agent Cognition Trace Interface）**，让人类以"观察者 / 审批者 / 训练者 / 监督者"身份，看到 Agent 如何**基于 Ontology 世界模型做决策**，而非仅仅看日志。本设计是 MVP 必须包含的可观察性层（Agent Observation Kernel）的详细规格。

## 2. 业务/技术背景
- 参照：`01_项目总览/project-overview.md`（Everything is Object、极简内核）、`03_架构设计/architecture-total.md`（Kernel + 插件化）、`03_架构设计/module-split.md`（crate 拆分）。
- 类比对象：IDE Debugger、Kubernetes Dashboard、LangSmith Trace、Palantir AIP 操作界面。
- 核心区别：不是看 Agent 的日志，而是看 Agent 如何基于 Ontology 世界模型决策。

## 3. 核心规则 & 约束（重点）
- [强制] Agent 的每一步（Goal/Intent/Query/Reasoning/Planning/Policy/Action/Memory）都必须绑定 Ontology 对象（`object_refs`），不可解释、无对象依据的决策禁止出现。
- [强制] 本层在 V0.3（=MVP）完整落地：接入 **Pi（`@earendil-works/pi-coding-agent`）作 Cognitive Engine** 作决策主体，三层图形 UI 暴露认知；V0.1 已先有 Event Bus + TraceEvent(world) + CLI 观察地基；高级自治（Evolution / 自学习 / Multi-Agent）延至 V0.5。
- [强制] 所有事件进入统一 `TraceEvent` 模型与 `wb_trace_event` 存储（`kind` 区分 world/cognitive），UI 三视图同源。
- [建议] 提供三层视图：Executive View（管理者）/ Operator View（管理员）/ Developer View（开发者）。
- [建议] 实时推送走 WebSocket `/api/v1/wb/agent/{id}/stream`，与审计、记忆、世界模型同源消费 Event Bus（详见 api-agent-trace.md 4.6）。

## 4. 详细设计/内容

### 4.1 设计目标重定义
传统系统人看结果；Agent 系统人应是**观察者、审批者、训练者、监督者**。

```
Human
  │
Observation Interface
  │
Agent Runtime
  │
Ontology Kernel
  │
World Model
  │
Reality
```

UI 核心闭环：**Observe → Understand → Intervene**。

### 4.2 核心组件：Agent Execution Timeline
最重要组件是 **Agent Trace Timeline**（类 Debugger / LangSmith）。示例：

```
任务: 降低客户流失风险         Agent: Retention-Agent        状态: Running
------------------------------------------------------------------
09:01:02  🎯 Goal Understanding
  目标解析: "降低客户流失"   Intent: Customer Retention   Confidence: 0.93
------------------------------------------------------------------
09:01:03  🌍 World Model Query
  查询 Ontology: Customer → Order → Payment
  发现: Customer_001   Risk: 0.72
------------------------------------------------------------------
09:01:05  🧠 Reasoning
  发现: 1.付款周期增加 2.订单下降 30% 3.投诉增加   推理: 可能流失
------------------------------------------------------------------
09:01:07  📋 Planning
  Step1 查询客户历史 / Step2 生成挽留方案 / Step3 创建销售任务
------------------------------------------------------------------
09:01:10  🔐 Policy Check
  Action: Create Sales Task   Policy: manager required   Result: ALLOW
------------------------------------------------------------------
09:01:12  ⚡ Action
  执行: CreateTask()   Result: Success
------------------------------------------------------------------
09:01:15  💾 Memory Update
  记录: Experience: Customer Retention Pattern #102
```

### 4.3 核心数据模型：TraceEvent（统一事件）
不直接显示 Log，而是结构化事件。Event 只有一种 `TraceEvent`，`kind` 区分两类：

- **world**（V0.1 起）：对象/关系生命周期与领域事件——世界"发生过什么"（`ObjectCreated/Updated/Deleted`、`RelationLinked`、`DomainEvent` 如 `PaymentLate`）。
- **cognitive**（V0.3 起）：Agent 认知事件——Agent"想过什么"（`Goal/Intent/OntologyQuery/Reasoning/Planning/PolicyCheck/Action/MemoryUpdate/Reflection`）。

```rust
pub struct TraceEvent {
    id: String,
    kind: TraceKind,                 // World | Cognitive
    timestamp: DateTime,
    event_type: TraceType,
    object_refs: Vec<ObjectRef>,     // 绑定 Ontology，可解释性根基
    // —— cognitive 专有（kind=Cognitive 时填充）——
    run_id: Option<String>,
    agent_id: Option<String>,
    reasoning: Option<String>,
    input: Option<Value>,
    output: Option<Value>,
    confidence: Option<f32>,
}

enum TraceKind { World, Cognitive }

enum TraceType {
    // world
    ObjectCreated, ObjectUpdated, ObjectDeleted, RelationLinked, DomainEvent,
    // cognitive
    Goal, Intent, OntologyQuery, Reasoning, Planning, PolicyCheck, Action, MemoryUpdate, Reflection,
}
```

world 事件让 V0.1 的 Event Bus 不空转（Object CRUD 即发）；cognitive 事件在 V0.3 复用同一模型与存储，仅 `run_id / agent_id / reasoning` 等字段按需填充。

### 4.4 与 Ontology Kernel 结合
Agent 每一步必须绑定 Ontology。普通 Agent 输出"我觉得客户危险"不可解释；OpenAIP-OS 输出：

```
Reasoning Evidence
┌─────────────────────────┐
│ Customer_001  Risk 0.72  │
│ Evidence:               │
│   PaymentLate ×3        │
│   Order ↓30%            │
│   Complaint ×5          │
└─────────────────────────┘
```
即 Reasoning 基于 `Object: Customer_001` + `Relations: Customer|Orders|Payments` + `Events: PaymentLate/OrderDecrease`。

### 4.5 三层观察界面
**Layer 1 Executive View（管理者）**：只看 Mission / Progress / Decision / Risk / Human Approval，不显示思考细节。

**Layer 2 Operator View（管理员）**：类 Kubernetes，显示 Agent 运行时状态与 Pipeline 进度（Goal ✓ → Intent ✓ → Ontology Query ✓ → Reasoning ▶ → Policy ○ → Action ○）。

**Layer 3 Developer View（开发者）**：类 Debugger，显示 Trace ID、Input、Ontology Context、Planner Output、Tool Call（如 `ontology.customer.query()`）。

### 4.6 关键组件
1. **Agent Timeline**：前端 React Flow / D3；结构 `AgentRun → { TraceEvent, Decision, Evidence, Action }`。
2. **Decision Graph**：决策是图非纯线性（类 LangGraph）。例：`Goal → Intent → ┬ Query Customer → Risk Model → Decision → Action` 与 `└ Query Orders`。
3. **World Model Explorer**：人也能看到 Agent 看到的世界（`Customer_001` 的 Properties / Relations / Events / Actions）。
4. **Agent Memory Viewer**：对应 V1.1 四类记忆——Working / Episodic / Semantic / Ontology Memory。

### 4.7 事件架构
Agent Runtime 发布 `RuntimeEvent` → Event Bus → 分发至 Trace Storage / UI Stream / Memory / Audit。

存储表 `wb_trace_event`：建表 DDL 以 `sql-script.md` 为唯一权威来源（V1.3.0 段），字段枚举值见 `data-dict.md`（trace_kind / trace_event_type）。架构文档不再内嵌 DDL，避免与 sql-script.md 漂移。

实时推送：

```text
WebSocket: /api/v1/wb/agent/{id}/stream   # {id} = runId 或 agentId，详见 api-agent-trace.md 4.6
{
  "id": "tr_abc123",
  "kind": "cognitive",
  "runId": "run_001",
  "agentId": "retention-agent",
  "timestamp": "2026-08-11T09:01:05Z",
  "eventType": "Reasoning",
  "objectRefs": ["Customer_001"],
  "reasoning": "Risk increased because payment delay",
  "confidence": 0.93
}
```

### 4.8 MVP 取舍
本层与 Agent 同在 V0.3（=MVP）完整落地：接入 **Pi（`@earendil-works/pi-coding-agent`）作 Cognitive Engine** 作决策主体，三层图形 UI 暴露其认知。V0.1 已先打地基（Event Bus + TraceEvent(world) + CLI 观察），保证 Event Bus 不空转、模型不割裂。实现：`Pi(Cognitive Engine) → OpenAIP Planner → TraceEvent(cognitive) → Timeline UI`：Agent 基于 Ontology 世界模型做 Goal→Query→Plan→Action，观察层暴露其认知过程。高级自治（Evolution / 自学习 / Multi-Agent）延至 V0.5。

### 4.9 推荐架构调整
原：`Ontology Kernel → Agent Runtime → UI`
调整（Human 在环）：

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

### 4.10 最终定位
OpenAIP-OS 不应只是"给 Agent 用的数据库"，而是**第一个 Agent Native 的可观察世界模型操作系统**。

核心能力：**Agent 可以行动 + 人可以理解 + 人可以干预 + 系统可以学习**。

这是区别于 LangChain / AutoGen / CrewAI 等普通 Agent Framework 的核心竞争力；当前 Ontology + Event + Runtime 天然适合承载这一层。

## 5. 风险点 & 注意事项
- 若 Trace 事件不绑定 `object_refs`，可解释性承诺落空，须评审卡点。
- MVP 已接入 Pi（Cognitive Engine）；仍须避免顺手实现 Evolution / 自学习等高级自治，相关边界见 `todo-backlog.md` BL-001。
- WebSocket 实时流与 `wb_trace_event` 持久化需保证同源、不丢审计。

## 6. 关联文档
- [./architecture-total.md](./architecture-total.md)
- [./module-split.md](./module-split.md)
- [./pi-agent-integration.md](./pi-agent-integration.md)
- [../01_项目总览/project-overview.md](../01_项目总览/project-overview.md)
- [../02_需求设计/requirement-all.md](../02_需求设计/requirement-all.md)
- [../09_迭代与版本/sprint-plan.md](../09_迭代与版本/sprint-plan.md)
- [../09_迭代与版本/todo-backlog.md](../09_迭代与版本/todo-backlog.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成（Agent Cognition Trace Interface 全套设计） |
| V1.1 | lushaohui | 2026-08-11 | 纠正：MVP 接入 PI Agent（LLM 驱动）；4.8 由"不做 Agent"改为"接入 PI Agent 作决策主体" |
| V1.2 | lushaohui | 2026-08-11 | Pi 降级为 Cognitive Engine；4.8 改为 Pi 经 AgentEngine 接入，OpenAIP 拥有 Planner/Policy |
| V1.3 | lushaohui | 2026-08-11 | 方案B对齐：AgentTraceEvent 泛化为统一 TraceEvent(kind=world/cognitive)；表泛化为 trace_event；MVP 定位归 V0.3，V0.1 先有 world 事件地基 |
| V1.4 | lushaohui | 2026-08-11 | 表名对齐 table-design 约束改为 wb_trace_event；4.7 DDL 补齐统一字段与索引，指向 sql-script/data-dict 为权威 |
| V1.5 | lushaohui | 2026-08-11 | 4.7 内嵌 DDL 抽离至 sql-script.md（单一权威），4.7 仅保留引用与实时推送示例 |
