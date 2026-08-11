# 验收标准
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-02-004 |
| 所属模块 | WorkBuddy-Requirement |
| 版本 | V1.6 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
定义各版本需求可被判定为「完成」的可量化条件，作为测试与上线卡点。MVP = V0.3；V0.1（REQ-001~008）为内核 + 观察基建，V0.3（REQ-009~011）为 Agent 闭环。

## 2. 业务/技术背景
- 依赖 `requirement-all.md`（REQ-001~011）、`prd-agent-observation.md`、`prd-agent-integration.md`。
- 与 `07_测试文档/test-case.md` 一一对应。

## 3. 核心规则 & 约束（重点）
- [强制] 每条验收标准必须可测试、可判定，禁止「基本可用」等模糊表述。
- [强制] 验收标准与需求编号一一对应。
- [强制] 事件缺 `object_refs`（未绑定 Ontology 对象）视为验收不通过（可解释性卡点）。

## 4. 详细设计/内容

### 4.1 V0.1 验收标准（REQ-001~008：Object Kernel + 观察基建）
| 需求编号 | 验收条件 | 验证方式 | 负责角色 |
|----------|----------|----------|----------|
| REQ-001 | 给定 `customer.odl`（name:string, level:string）能被解析为 IR；缺失 property 或使用未知类型时返回明确错误码 | 自动化编译测试 | 编译器研发 |
| REQ-002 | 生成的 IR 含 `ObjectType` / `PropertyDef`，字段名与类型与 .odl 完全一致，无信息丢失 | 单测断言 IR 结构 | 编译器研发 |
| REQ-003 | 生成 `Customer.rs`（pub struct + `CUSTOMER_DEF` const）、`schema.sql`、`ontology.json`，三者与 IR 一致且可编译 | 产物编译 + diff | Codegen 研发 |
| REQ-004 | `ObjectEngine` 的 create/get/update/query 对 `wb_ontology_object` 生效；update 使 version 自增；query 支持按 type 过滤 | 集成测试 | Runtime 研发 |
| REQ-005 | `LibSQLStorage` 在 Turso 上读写成功，落库数据可被 query 取回 | 集成测试 | Storage 研发 |
| REQ-006 | `oaip compile / run / object list / event list` 跑通端到端示例，`oaip object list Customer` 输出 `001 Apple VIP` / `002 Tesla GOLD` | e2e 脚本 | CLI 研发 |
| REQ-007 | Object CRUD 即产生 `TraceEvent(kind=world)`（eventType=ObjectCreated/Updated/Deleted），绑定 `object_refs` 并落 `wb_trace_event` 表；缺 `object_refs` 的事件被拒绝记录 | 集成测试断言事件数=操作数且均含 object_refs | 观察层研发 |
| REQ-008 | `RuntimeEvent` 经 Event Bus 分发，`wb_trace_event` 表写入对应记录；`oaip event list` 可查看 world 事件 | 集成测试断言存储=发布 + CLI 验证 | 观察层研发 |

### 4.2 V0.3 验收标准（REQ-009~011：Agent 闭环 = MVP）
| 需求编号 | 验收条件 | 验证方式 | 负责角色 |
|----------|----------|----------|----------|
| REQ-009 | Executive / Operator / Developer 三视图可渲染；`WebSocket /api/v1/wb/agent/{id}/stream` 实时推送 cognitive 事件；Timeline 展示 Goal→Intent→OntologyQuery→Reasoning→Planning→PolicyCheck→Action→MemoryUpdate 链；断线重连可补齐历史 | 前端 e2e + ws 客户端测试 | 前端研发 |
| REQ-010 | Pi（AgentEngine trait）接入后，OpenAIP Planner 调用其 `reason()` 完成 NLU/推理；Pi 每步认知经 Event Bus 产出 `TraceEvent(kind=cognitive)` 且绑定 `object_refs`；Observation UI 能回放其认知过程 | Mock 目标 + 断言 cognitive trace 含 Agent 决策且均带 object_refs | 观察层研发 |
| REQ-011 | OpenAIP Planner 跑通 Goal→LLMGoalParser→Planner→PlanStep→Capability→Execute；PlanStep 可经 Trace 回放且绑定 `object_refs` | 集成测试断言 PlanStep 链路 + trace | Runtime 研发 |

### 4.3 端到端验收基线
- **V0.1 基线**：`customer.odl` 跑通 `compile → run → object list → event list`，world 事件可回放，即 V0.1 通过。
- **V0.3（MVP）基线**：`oaip agent run "reduce churn"` 跑通 `Goal → OntologyQuery → Reasoning → Planning → Action`，Observation UI 回放完整 cognitive Trace 且均绑定 `object_refs`，即 MVP 通过。

## 5. 风险点 & 注意事项
- 验收标准缺失会使上线变得主观；新增需求须同步补此表。
- 可解释性是差异化卖点，缺 `object_refs` 必须卡回，不能因赶工期放行。

## 6. 关联文档
- [../02_需求设计/requirement-all.md](../02_需求设计/requirement-all.md)
- [../02_需求设计/prd-agent-observation.md](../02_需求设计/prd-agent-observation.md)
- [../02_需求设计/prd-agent-integration.md](../02_需求设计/prd-agent-integration.md)
- [../03_架构设计/agent-observation-design.md](../03_架构设计/agent-observation-design.md)
- [../07_测试文档/test-plan.md](../07_测试文档/test-plan.md)
- [../07_测试文档/test-case.md](../07_测试文档/test-case.md)
- [../09_迭代与版本/task-spec-template.md](../09_迭代与版本/task-spec-template.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成 |
| V1.1 | lushaohui | 2026-08-11 | 填入 V0.1 验收标准（REQ-001~009）与端到端基线 |
| V1.2 | lushaohui | 2026-08-11 | 新增 REQ-010 验收标准（PI Agent 接入，基于世界模型决策且 trace 绑定 object_refs） |
| V1.3 | lushaohui | 2026-08-11 | REQ-010 改指 Pi Cognitive Engine（AgentEngine）+ OpenAIP Planner；新增 REQ-011 Planner 验收 |
| V1.4 | lushaohui | 2026-08-11 | 按版本分组验收：V0.1（REQ-001~008 world 事件 + Event Bus）、V0.3=MVP（REQ-009~011 Agent 闭环）；TraceEvent 模型统一 |
| V1.5 | lushaohui | 2026-08-11 | 关联补 test-plan 与 task-spec-template（TDD 闭环打通） |
| V1.6 | lushaohui | 2026-08-11 | 表名对齐 table-design 约束：`trace_event` → `wb_trace_event` |
