# 需求/任务待办池
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-09-003 |
| 所属模块 | WorkBuddy-Version |
| 版本 | V1.5 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
沉淀 MVP 阶段**明确推迟或排除**的能力，说明推迟理由与计划引入版本，防止在 Kernel 阶段被误提前实现。

## 2. 业务/技术背景
- 依据 `01_项目总览/project-overview.md` 的最大风险判断：没有世界模型，上层能力只是 Prompt 工程。
- 与 `09_迭代与版本/sprint-plan.md` 互补：本文件记"暂不做"，sprint-plan 记"做"。

## 3. 核心规则 & 约束（重点）
- [强制] 被排除能力在对应版本前不得进入 MVP 代码，违者评审卡回。
- [建议] 每条待办标注优先级与计划版本，便于排期。

## 4. 详细设计/内容

### 4.1 被推迟 / 排除能力
| 编号 | 能力 | 推迟理由 | 计划版本 |
|------|------|----------|----------|
| BL-001 | Agent 高级自治（自学习 / Evolution / Memory 沉淀） | 基础 Agent 接入（Pi Cognitive Engine + Planner）在 V0.3 落地（=MVP，REQ-010/011）；Memory 沉淀已在 V0.5 落地（REQ-016，wb_memory + MemoryRecall 闭环），Evolution 仍需 Experience 积累 | V0.5（Memory 已实现）/ 远期（Evolution） |
| BL-002 | GraphRAG | 是 Ontology + Knowledge Graph + Embedding 的上层；无 Object Graph 无意义 | 世界模型成熟后 |
| BL-003 | Evolution Engine | 需 Experience→Pattern→Proposal→Validation→Merge；MVP 连 Experience 都无 | 远期 |
| BL-004 | Multi-Agent | 属 Agent Society（Protocol/Consensus/Delegation） | V3 |
| BL-005 | 完整 Action Engine（对象自有能力 actions） | 最小 Action 执行已在 V0.3 随 Planner 闭环落地；完整引擎=Object 的未来行为，需先有稳定世界模型 | V0.4 |
| BL-006 | Policy Engine | = Action 的约束，依赖 Action | V0.4 已实现（动作级 `policy:` 审批闸门 + 前置条件 + CLI 交互审批 + server 挂起式 HTTP 审批端点，V0.4 遗留项已收口） |
| BL-007 | Workflow | 依赖 Action / Policy 编排 | 远期 |
| BL-008 | Memory | = Event 的积累，依赖 World Model 领域事件（V0.2）与认知事件（V0.3）沉淀 | V0.5（后端 wb_memory 沉淀 + `MemoryRecall` 闭环已实现；观察 UI Memory 视图已接入 `GET /api/v1/wb/agent/memory`） |
| BL-009 | Knowledge Graph | 是 GraphRAG 底座，依赖 Object Graph | 世界模型成熟后 |

### 4.2 推迟原则
**没有世界模型，上层能力无意义。** 所有 BL 项都建立在 Object / Relation / Event 构成的 World Model 之上，必须由 V0.1~V0.2 先行夯实；Agent 闭环（V0.3=MVP）在其上生长。

## 5. 风险点 & 注意事项
- 待办长期不清理会失去规划价值，建议每个版本评审时回看本表。
- 若将 BL 项误标为"已做"，会造成路线图与实际严重偏离。

## 6. 关联文档
- [../02_需求设计/requirement-all.md](../02_需求设计/requirement-all.md)
- [../09_迭代与版本/sprint-plan.md](../09_迭代与版本/sprint-plan.md)
- [../01_项目总览/project-overview.md](../01_项目总览/project-overview.md)
- [../03_架构设计/agent-observation-design.md](../03_架构设计/agent-observation-design.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成 |
| V1.1 | lushaohui | 2026-08-11 | 填入被推迟/排除能力清单（Agent/GraphRAG/Evolution/Multi-Agent 等）及推迟原则 |
| V1.2 | lushaohui | 2026-08-11 | BL-001 拆分为：可观察层（MVP 已做）vs Agent 智能运行时（延至 V0.5） |
| V1.3 | lushaohui | 2026-08-11 | 纠正：基础 Agent 接入（Pi Cognitive Engine）已纳入 MVP（REQ-010）；BL-001 改为仅指高级自治（自学习/Evolution） |
| V1.4 | lushaohui | 2026-08-11 | Pi 降级为 Cognitive Engine；BL-001 描述同步（基础接入=Pi Cognitive Engine） |
| V1.5 | lushaohui | 2026-08-11 | 方案B对齐：BL-001 基础接入归 V0.3（=MVP）；BL-005 Action 分两层（V0.3 最小执行 / V0.4 完整引擎）；BL-008 Memory 归 V0.5 |
