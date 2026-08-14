# 统一术语字典
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-01-004 |
| 所属模块 | OpenAIP-OS-Overview |
| 版本 | V1.1 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
收敛项目内所有中英文术语、缩写与别名，避免命名混乱与沟通歧义。

## 2. 业务/技术背景
被所有业务/技术文档引用，是命名一致性的唯一权威来源。

## 3. 核心规则 & 约束（重点）
- [强制] 任何文档出现新术语必须先在此登记，禁止文档内私自定义缩写。
- [强制] 数据库表名、接口字段名必须与术语字典含义一致。
- [建议] 每条术语标注英文原名与适用模块。

## 4. 详细设计/内容
### 4.1 术语表
| 中文术语 | 英文/缩写 | 定义 | 适用模块 | 易混淆项 |
|----------|-----------|------|----------|----------|
| 本体操作系统 | Ontology OS / OpenAIP-OS | 第一个 Agent Native 的可观察世界模型操作系统（Good Admin） | 全局 | 非普通 Agent 框架 |
| 对象 | Object | 世界中可被定义与操作的实体；Everything is Object 的核心单元 | Ontology Kernel | 非禁用词，见 4.2 |
| 本体内核 | Ontology Kernel | 极简运行时内核：ObjectDef / ObjectInstance / Relation / Event / Storage | Runtime | 不含业务特化 Engine |
| 对象定义 | ObjectDef | 类型 schema，由 ODL 编译生成，驱动运行时行为（metadata-as-const） | Compiler / Runtime | 非数据库表 |
| 对象实例 | ObjectInstance | 现实对象，持久化于 `wb_ontology_object` | Runtime / Storage | 一行 JSON 记录 |
| 关系 | Relation | 世界连接，持久化于 `wb_relation_edge`（source-relation-target） | World Model | 非外键约束 |
| 事件 | Event | 时间维度描述「发生过什么」，统一为 TraceEvent | Trace | 非日志 |
| 统一轨迹事件 | TraceEvent | 唯一事件模型，`kind ∈ {world, cognitive}`，存 `wb_trace_event` | Trace | world=对象生命周期，cognitive=Agent 认知 |
| 世界模型 | World Model | Object + Relation + Event 聚合的可查询状态 | World Model | 非知识图谱本体 |
| 认知引擎 | Cognitive Engine | 提供推理/NLU/工具调用的可替换引擎（Pi/Claude/Qwen/LocalLLM） | Agent | 非 Agent 本体 |
| Pi | `@earendil-works/pi-coding-agent` | MVP 默认 Cognitive Engine 实现，经 `AgentEngine` trait 接入 | Agent | 可替换为其他引擎 |
| 引擎接口 | AgentEngine | `fn reason(ctx) -> Decision`，OpenAIP 与具体 LLM 的解耦边界 | Agent | trait，非具体实现 |
| 规划器 | Planner | OpenAIP 自有核心：Goal→LLMGoalParser→PlanStep→Capability→Execute | Agent | 非 Pi 提供 |
| 策略 | Policy | Action 约束与审批（Human Approval），V0.4 完整 | Agent | 非权限模型 |
| 本体中间表示 | Ontology IR | 语言无关单一真相源，由 .odl 编译产出 | Compiler | 非最终代码 |
| 代码生成 | Codegen | 由 IR 生成 Rust struct / schema.sql / ontology.json | Compiler | 产物受 IR 约束 |
| 本体定义语言 | ODL | `.odl` 源文本，`object X: properties: 块` 定义类型 | Compiler | 属性语法见 architecture-total 4.2.1 |
| 观察界面 | Observation UI | 三层视图（Executive/Operator/Developer）+ Trace API + WebSocket | Trace | 非日志面板 |

### 4.2 命名禁用清单
- 禁止用含义模糊的词：数据、信息（须具体到业务实体）。
- 注：`对象（Object）` 是本项目核心哲学术语（Everything is Object），允许并鼓励使用，不受禁用约束；禁止以「数据/信息」代指对象。


## 5. 风险点 & 注意事项
- 术语漂移会在代码与文档间产生长期割裂，需定期回顾。

## 6. 关联文档
- [../01_项目总览/project-constraint.md](../01_项目总览/project-constraint.md)
- [../04_数据库设计/data-dict.md](../04_数据库设计/data-dict.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成 |
| V1.1 | lushaohui | 2026-08-11 | 填充 4.1 术语表（Ontology OS / Object / TraceEvent / Cognitive Engine / Pi 等 16 条）；4.2 移除「对象」禁用（核心哲学术语） |

