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
- Policy 引擎（**已实现**）：动作级 `policy:` 约束 —— `require_approval` 审批闸门（默认 Deny→WB2005，转 Human Approval，经 `PolicyApprover` 放行）+ `precondition` 前置条件（字段/参数比较，AND）；需求 REQ-015，TC-015a/b/c/d 全绿
- 交互式 Human Approval（**已实现**）：CLI `agent run` 默认 `ConsoleApprover`（stdin 交互 y/N，按 `action@target` 缓存决策）+ `--auto-approve` 开关；server `serve` 同步加 `--auto-approve`（`AutoApprover`）；TC-021a/b 全绿
- server 挂起式 HTTP 审批端点（**已实现**，V0.4 遗留项）：`require_approval` 动作触发时运行挂起，注册待审批请求并发 `PendingApproval` 认知事件；浏览器经 `GET /run/:run_id/approvals` 拉取、`POST /run/:run_id/approval/:req_id` 裁决（approve→恢复执行 / reject→WB2005 终止，已裁决请求拒绝二次裁决）；新增 `ServerApprover`+`ApprovalStore`，`PolicyApprover::approve` 升为 async；TC-023a/b 全绿

**V0.5 Agent 自治**（让系统能学习）
- Memory 沉淀（**已实现**）：经验按 `action.<name>` 沉淀至 `wb_memory`（hits 累加），下一轮 run 经 `MemoryRecall` 认知事件召回，形成 Action→Memory→下次 Plan 闭环；需求 REQ-016，TC-022a/b 全绿
- 观察 UI 接入 Memory 视图（**已实现**）：三层认知导航新增 Memory 标签，前端经 `GET /api/v1/wb/agent/memory` 渲染沉淀列表（label/payload/hits/updatedAt），运行前载入、收到 MemoryUpdate/MemoryRecall 事件实时刷新；TC-024a 全绿
- Evolution Engine MVP（**已实现**）：BL-003，确立第一原则 **Learn≠Permission**（Evolution 不修改 Policy、不绕过 Human Approval）。`wb_memory` 经验 → Pattern(hits≥2, confidence=min(1,hits/5)) → Proposal(kind=ActionPattern, status=Proposed) → Validation(阈值≥0.6→Validated) → LearnedKnowledge，全程可追溯（run_ids/proposal_id/pattern_label）；`run_evolution` 幂等，server 暴露 `GET /api/v1/wb/agent/evolution`；TC-025a~h 全绿。AutoApprove 类「Knowledge→Permission」升级归第二阶段（需 Policy 显式允许学习 + 高置信 + 多次成功 + 无失败 + 低风险）

### 4.3 范围与排期
| 版本 | 目标 | 关联需求 | 状态 |
|------|------|----------|------|
| V0.1 | Object Kernel + 生命周期事件 + CLI 观察 | REQ-001~008 | 已实现 |
| V0.2 | World Model（Relation + 领域 Event） | REQ-012/013 | 已实现 |
| V0.3 | Agent 闭环（= MVP） | REQ-009~011 | 已实现 |
| V0.4 | Action & Policy | REQ-014（Action 引擎）/ REQ-015（Policy 引擎） | 已实现 |
| V0.5 | Agent 自治（Memory 沉淀 + Evolution MVP） | REQ-016（已实现）；REQ-017 Evolution Engine MVP（已实现，Learn≠Permission） | 已实现 |
| V0.6 | Evolution 第二阶段 AutoApprove（受控 Knowledge→Permission） | REQ-018（规划中：5 道闸门 + 默认关闭 + 可审计 + 可撤销；新增 ActionFailed 埋点） | 规划中（TDD Contract 已出） |

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
| V2.1 | lushaohui | 2026-08-12 | V0.4 Policy 引擎落地：REQ-015 已实现（状态表 V0.4→已实现）；动作级 `policy:` 审批闸门 + 前置条件；TC-015a/b/c/d 全绿 |
| V2.2 | lushaohui | 2026-08-12 | 交互式 Human Approval 落地：CLI `ConsoleApprover` + server `--auto-approve` 开关；TC-021a/b 全绿 |
| V2.3 | lushaohui | 2026-08-12 | V0.5 Memory 沉淀落地：REQ-016 已实现（状态表 V0.5→已实现）；经验按 `action.<name>` 沉淀 `wb_memory` + `MemoryRecall` 闭环；Evolution 归远期；TC-022a/b 全绿 |
| V2.4 | lushaohui | 2026-08-12 | V0.4 遗留项：server 挂起式 HTTP 审批端点落地；`PolicyApprover::approve` 升 async，新增 `ServerApprover`+`ApprovalStore`，server 暴露 `GET /run/:run_id/approvals` 与 `POST /run/:run_id/approval/:req_id`（approve→恢复 / reject→WB2005 终止）；TC-023a/b 全绿 |
| V2.5 | lushaohui | 2026-08-12 | 观察 UI 接入 Memory 视图：后端新增 `GET /api/v1/wb/agent/memory`（`recall_all` 按 hits 降序）；前端三层导航加 Memory 标签渲染沉淀列表，运行前载入 + MemoryUpdate/MemoryRecall 事件实时刷新；TC-024a 全绿 |
| V2.6 | lushaohui | 2026-08-12 | Evolution Engine MVP（REQ-017）落地：Learn≠Permission；`src/evolution.rs` + storage 三表 `wb_pattern/wb_proposal/wb_learned` + RuntimeKernel `run_evolution` + server `GET /api/v1/wb/agent/evolution`；TC-025a~h 全绿（含 Isolation 不绕过审批、幂等、可追溯） |
| V2.7 | lushaohui | 2026-08-12 | 规划 Evolution 第二阶段 AutoApprove（REQ-018）：范围表（4.3）加 V0.6 行；TDD Contract 见 task-spec-evolution-autoapprove.md；5 道闸门 + 默认关闭 + 可审计 + 可撤销，TC-026a~h 验收矩阵同步 |
| V2.8 | lushaohui | 2026-08-12 | 修复 server 挂起式 HTTP 审批超时残留（Critical-1/2/3）：`ApprovalState` 加 `Expired`；`ApprovalStore::expire` 超时回收 + `resolve` 对 Expired 返回 false；`ServerApprover` 超时 emit Expired（与 Rejected 区分）；新增 TC-023c/d 全绿 |
