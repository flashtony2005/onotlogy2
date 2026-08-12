# 迭代计划
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-09-001 |
| 所属模块 | WorkBuddy-Version |
| 版本 | V1.8 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
规划 OpenAIP-OS 从极简内核分形生长到完整 Ontology OS 的版本演进（V0.1~V0.5），明确每版目标、边界与需求归属，保证每个版本都是一个可独立跑通的最小闭环。

## 2. 业务/技术背景
- 依赖 `01_项目总览/project-overview.md` 的 MVP 战略与 `03_架构设计/architecture-total.md` 的极简三原则。
- 被推迟能力见 `todo-backlog.md`。

## 3. 核心规则 & 约束（重点）
- [强制] **MVP = V0.3**。V0.1/V0.2 是内核与世界模型的最小证明，不对外称 MVP。
- [强制] 严格按 V0.1→V0.5 顺序生长，前序版本是后序版本的世界模型基础；禁止跨版本提前引入能力。
- [强制] 每个版本须端到端跑通一个最小闭环，不是半成品。
- [建议] 关联需求编号（见 requirement-all.md），保证可追溯。

## 4. 详细设计/内容

### 4.1 演进路线总览
```
V0.1 Object Kernel   让对象有生命      .odl→IR→Rust→Turso→CRUD + 生命周期事件
V0.2 World Model     让世界成型        + Relation + 领域 Event（三要素闭环）
V0.3 Agent 闭环(MVP) 让 Agent 住进世界  + Pi Cognitive Engine + Planner + 最小 Action + 观察 UI
V0.4 Action & Policy 让行为可编排       完整 Action Engine（已实现）+ Policy 约束（待办）
V0.5 Agent 自治      让系统能学习       自学习 / Evolution / Memory 沉淀
```

### 4.2 各版本范围
**V0.1 Object Kernel**（让对象有生命）
- 闭环：`.odl → IR → Rust → Turso → Object CRUD`
- 交付：ODL、Compiler、Codegen、ObjectEngine、LibSQLStorage、oaip-cli
- 观察基建：Event Bus + `TraceEvent(kind=world)`（对象生命周期：created/updated/deleted）+ `wb_trace_event` 表；**观察走 CLI**（`oaip object list` / `oaip event list`），无图形 UI
- 边界：无 Relation、无领域 Event 引擎、无 Agent、无图形 UI
- 需求：REQ-001~008

**V0.2 World Model**（让世界成型）
- 增加 Relation：`wb_relation_edge`，支持 `Customer --owns--> Account` 等图查询（运行时 API，不扩展 ODL）
- 增加领域 Event：`PaymentLate` 等业务事件（`TraceEvent kind=world` 的语义扩展，复用统一事件模型）
- 闭环：Object + Relation + Event 三要素聚合为可查询的 World Model
- 需求：REQ-012（Relation 引擎）/ REQ-013（领域 Event）

**V0.3 Agent 闭环（= MVP，让 Agent 住进世界并被人理解）**
- 接入 **Pi 作 Cognitive Engine**（`AgentEngine` trait）+ OpenAIP 自有 **Planner**（Goal→LLMGoalParser→Planner→PlanStep→Capability→Execute）
- 最小 **Action 执行**（让 Planner 闭环不断裂；完整 Action Engine 已在 V0.4 落地）
- `TraceEvent(kind=cognitive)`：Agent 认知事件（Goal/Intent/Reasoning/Planning/...），绑定 `object_refs`
- 三层图形 **Observation UI**（Executive/Operator/Developer）+ WebSocket 流（详设 `agent-observation-design.md`）
- 端到端：用户输入 → Pi(NLU) → Intent → Ontology Query → Planner → Action → Executor → Trace UI
- 需求：REQ-009/010/011

**V0.4 Action & Policy**（让行为可编排）
- Action 从"最小执行"升级为完整引擎（**已实现**）：对象自有能力经 ODL `actions:` 声明 → `ActionRegistry` → `ActionEngine` 数据驱动解释执行（`updates` / `creates` / `link`，幂等），替代硬编码挽留计划；需求 REQ-014，TC-014a/b/c 全绿
- Policy 约束与审批（Human Approval）：**待办**，归后续版本（V0.5 或单独 Policy 迭代）

**V0.5 Agent 自治**（让系统能学习）
- 自学习 / Evolution / Memory 沉淀
- 生命周期（沿用 V1.1）：`Goal → Intent → Plan → Query → Reasoning → Policy → Action → Memory`

### 4.3 范围与排期
| 版本 | 目标 | 关联需求 | 状态 |
|------|------|----------|------|
| V0.1 | Object Kernel + 生命周期事件 + CLI 观察 | REQ-001~008 | 已实现 |
| V0.2 | World Model（Relation + 领域 Event） | REQ-012/013 | 已实现 |
| V0.3 | Agent 闭环（= MVP） | REQ-009~011 | 已实现 |
| V0.4 | Action & Policy | REQ-014（Action 引擎已实现；Policy 待办） | 部分实现 |
| V0.5 | Agent 自治 | 待拆 | 未开始 |

## 5. 风险点 & 注意事项
- 范围蔓延：V0.3 的 Agent 极易被提前"顺手"塞进 V0.1/V0.2，须以本表为卡点。
- Event Bus 在 V0.1 只承载 `world` 事件；若 V0.1 无 `WorldEvent` 产生则属空转，须保证 Object CRUD 即发事件。
- 前序版本若不稳固（如 IR 定义漂移），会放大到后续所有版本。

## 6. 关联文档
- [../01_项目总览/project-overview.md](../01_项目总览/project-overview.md)
- [../02_需求设计/requirement-all.md](../02_需求设计/requirement-all.md)
- [../03_架构设计/architecture-total.md](../03_架构设计/architecture-total.md)
- [../03_架构设计/agent-observation-design.md](../03_架构设计/agent-observation-design.md)
- [../09_迭代与版本/todo-backlog.md](../09_迭代与版本/todo-backlog.md)
- [../09_迭代与版本/version-changelog.md](../09_迭代与版本/version-changelog.md)
- [../09_迭代与版本/task-spec-template.md](../09_迭代与版本/task-spec-template.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成 |
| V1.1 | lushaohui | 2026-08-11 | 填入 V0.1~V0.5 演进路线与版本范围 |
| V1.2 | lushaohui | 2026-08-11 | V0.1 纳入 Agent Observation Kernel（可观察性层） |
| V1.3 | lushaohui | 2026-08-11 | 纠正：V0.1 纳入 Agent 接入（PI Agent）；V0.5 改为 Agent 自治扩展 |
| V1.4 | lushaohui | 2026-08-11 | Pi 降级为 Cognitive Engine；V0.1 增 Planner（REQ-011），范围扩为 Pi Cognitive Engine + Planner |
| V1.5 | lushaohui | 2026-08-11 | 重切版本解耦：V0.1 回归极简内核（WorldEvent+CLI 观察）、V0.2 世界模型、V0.3=MVP Agent 闭环；Event 统一为 TraceEvent(world/cognitive)；REQ-009~011 移至 V0.3 |
| V1.6 | lushaohui | 2026-08-11 | 关联补 task-spec-template（TDD 闭环打通） |
| V1.7 | lushaohui | 2026-08-11 | 表名对齐 table-design 约束：`trace_event` → `wb_trace_event` |
| V1.8 | lushaohui | 2026-08-11 | V0.2 落地：范围补 REQ-012/013；状态 未开始→已实现 |
| V1.9 | lushaohui | 2026-08-12 | V0.3=MVP 落地：状态 未开始→已实现 |
| V2.0 | lushaohui | 2026-08-12 | V0.4 完整 Action 引擎落地：REQ-014 已实现（状态表 V0.4→部分实现）；Policy 审批标记待办 |
