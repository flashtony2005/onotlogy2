# 数据字典与枚举
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-04-003 |
| 所属模块 | WorkBuddy-DB |
| 版本 | V1.2 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
集中管理所有枚举值、状态码，作为代码与文档的唯一对应来源。

## 2. 业务/技术背景
被 table-design.md 与全部接口文档引用。

## 3. 核心规则 & 约束（重点）
- [强制] 所有枚举/状态码在此唯一定义，代码常量必须与文档值完全一致。
- [强制] 枚举变更须同时更新引用方文档与代码，并登记修订日志。

## 4. 详细设计/内容
### 4.1 枚举值表
| 枚举名 | 值 | 含义 | 使用表/字段 |
|--------|----|------|-------------|
| is_delete | 0 | 未删除 | 全表 |
| is_delete | 1 | 已删除 | 全表 |
| trace_kind | world | 世界事件：对象/关系生命周期与领域事件 | wb_trace_event.kind |
| trace_kind | cognitive | 认知事件：Agent 思考/决策过程 | wb_trace_event.kind |
| trace_event_type | ObjectCreated | 对象创建（world） | wb_trace_event.event_type |
| trace_event_type | ObjectUpdated | 对象更新（world） | wb_trace_event.event_type |
| trace_event_type | ObjectDeleted | 对象删除（world） | wb_trace_event.event_type |
| trace_event_type | RelationLinked | 关系建立（world） | wb_trace_event.event_type |
| trace_event_type | DomainEvent | 领域事件如 PaymentLate（world） | wb_trace_event.event_type |
| trace_event_type | Goal | 目标理解（cognitive） | wb_trace_event.event_type |
| trace_event_type | Intent | 意图解析（cognitive） | wb_trace_event.event_type |
| trace_event_type | OntologyQuery | 本体查询（cognitive） | wb_trace_event.event_type |
| trace_event_type | Reasoning | 推理（cognitive） | wb_trace_event.event_type |
| trace_event_type | Planning | 规划（cognitive） | wb_trace_event.event_type |
| trace_event_type | PolicyCheck | 策略校验（cognitive） | wb_trace_event.event_type |
| trace_event_type | Action | 动作执行（cognitive） | wb_trace_event.event_type |
| trace_event_type | MemoryUpdate | 记忆更新（cognitive） | wb_trace_event.event_type |
| trace_event_type | Reflection | 反思（cognitive） | wb_trace_event.event_type |
| proposal_status | Proposed | 提案初始态（Pattern→Proposal 后） | wb_proposal.status |
| proposal_status | Validated | 校验通过（confidence≥0.6）→ 可沉淀为 LearnedKnowledge | wb_proposal.status |
| proposal_status | Rejected | 校验未通过（confidence<0.6） | wb_proposal.status |
| proposal_kind | ActionPattern | 动作模式提案/知识（Pattern 描述某动作的复现规律） | wb_proposal.kind / wb_learned.kind |

### 4.2 状态码
| 状态码 | 含义 |
|--------|------|
|        |      |


## 5. 风险点 & 注意事项
- 枚举增删需全局检索引用，避免漏改。

## 6. 关联文档
- [../04_数据库设计/table-design.md](../04_数据库设计/table-design.md)
- [../05_接口文档/error-code.md](../05_接口文档/error-code.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成 |
| V1.1 | lushaohui | 2026-08-11 | 新增 trace_kind、trace_event_type 枚举（wb_trace_event 使用） |
| V1.2 | lushaohui | 2026-08-12 | 新增 proposal_status（Proposed/Validated/Rejected）、proposal_kind（ActionPattern）枚举（wb_proposal / wb_learned 使用，BL-003·REQ-017） |

