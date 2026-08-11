# 版本变更日志
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-09-002 |
| 所属模块 | WorkBuddy-Version |
| 版本 | V1.1 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
按版本倒序记录新增/修复/优化，作为上线核对依据。

## 2. 业务/技术背景
被 deploy-steps.md 与 rollback-plan.md 引用。

## 3. 核心规则 & 约束（重点）
- [强制] 按版本倒序，固定三级分类：新增功能 / Bug 修复 / 优化调整。
- [强制] 版本号规则：Vx.y.z（主版本。功能迭代。Bug 修复）。

## 4. 详细设计/内容
> 产品版本采用 V0.1~V0.5 路线（见 sprint-plan.md），以下按产品里程碑记录，固定三级分类：新增功能 / Bug 修复 / 优化调整。

## V0.3.0（MVP，规划中）
### 新增
1. Agent 闭环：Pi 作 Cognitive Engine（经 `AgentEngine` trait）+ OpenAIP 自有 Planner（Goal→LLMGoalParser→PlanStep→Capability→Execute）
2. 三层观察 UI（Executive / Operator / Developer View）+ WebSocket 实时轨迹流（`/api/v1/wb/agent/{id}/stream`）
3. `TraceEvent(kind=cognitive)` 模型 + Trace API（`api-agent-trace.md`）
### 优化
1. 统一 `TraceEvent` 模型（world / cognitive 同源），观察层与持久化同源消费 Event Bus

## V0.2.0（规划中）
### 新增
1. 关系边 `wb_relation_edge`，支撑图查询（`Customer --owns--> Account`）
2. 领域事件（如 `PaymentLate`）扩展 world 事件语义，World Model 三要素成型

## V0.1.0（规划中）
### 新增
1. 极简 Ontology Kernel：`.odl → Compiler → IR → Runtime → Turso → Object CRUD`
2. `wb_ontology_object` 统一对象存储（Everything is Object，禁止按业务建独立表）
3. Event Bus + `TraceEvent(kind=world)` + `wb_trace_event` + CLI 观察（`oaip object list` / `oaip event list`）


## 5. 风险点 & 注意事项
- changelog 与实际发布不符会使回滚失准。

## 6. 关联文档
- [../08_部署运维/deploy-steps.md](../08_部署运维/deploy-steps.md)
- [../08_部署运维/rollback-plan.md](../08_部署运维/rollback-plan.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成 |
| V1.1 | lushaohui | 2026-08-11 | 重写 changelog 为 OpenAIP-OS V0.1~V0.5 产品路线（替换通用模板的用户登录/任务CRUD内容） |

