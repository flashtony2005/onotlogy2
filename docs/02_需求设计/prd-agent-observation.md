# PRD：Agent Observation Kernel（智能体认知过程观察层）
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-02-005 |
| 所属模块 | WorkBuddy-Requirement |
| 版本 | V1.4 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
将 REQ-007 / REQ-008（V0.1 观察基建）与 REQ-009（V0.3 三层图形 UI）展开为可实现的详细 PRD，定义功能、角色、业务规则与异常边界，驱动 `03_架构设计/agent-observation-design.md` 与接口文档。

## 2. 业务/技术背景
- 来源：`requirement-all.md`（REQ-007/008/009）、`03_架构设计/agent-observation-design.md`。
- 关联：`03_架构设计/architecture-total.md`（4.6 可观察性架构）、`03_架构设计/module-split.md`（runtime/trace 模块）。

## 3. 核心规则 & 约束（重点）
- [强制] PRD 文件名规则：`prd-<模块英文短名>.md`，全小写短横线（本文件 `prd-agent-observation.md`）。
- [强制] 必须含：功能描述、交互流程、业务规则、异常分支、埋点/统计要求。
- [强制] 每个 Trace 事件必须绑定 Ontology 对象（`object_refs`），不可解释决策禁止进入系统。
- [建议] 附流程图（图片统一放 `/docs/assets/`）。

## 4. 详细设计/内容

### 4.1 功能概述
本层分两步落地：V0.1 观察基建（Event Bus + TraceEvent(world) + CLI 观察），V0.3（=MVP）图形观察层配合 **Pi（`@earendil-works/pi-coding-agent`）作 Cognitive Engine** 共同工作：OpenAIP 拥有 Agent Runtime（Planner/Policy），人类以观察者/审批者/训练者/监督者身份，看到 Agent 如何基于 Ontology 世界模型决策。核心交付（V0.3）：Trace Event 采集、Timeline UI、Decision Graph、World Model Explorer、Memory Viewer、三层视图、WebSocket 实时流。

### 4.2 用户角色与权限
| 角色 | 视角 | 权限要求 |
|------|------|----------|
| 管理者 Executive | 只看 Mission/Progress/Decision/Risk/Approval | 登录用户 |
| 管理员 Operator | Runtime 状态 + Pipeline 进度 | 登录用户 |
| 开发者 Developer | Trace 详情 / Ontology Context / Tool Call | 登录用户 |

### 4.3 功能详情
| 功能点 | 描述 | 优先级 | 验收标准 |
|--------|------|--------|----------|
| Trace Event 采集 | 每步产出 `TraceEvent`（含 `object_refs`，kind 区分 world/cognitive）经 Event Bus 入 `wb_trace_event` | Must | REQ-007/008 |
| Agent Timeline | 按时间轴展示 Goal→Intent→OntologyQuery→Reasoning→Planning→PolicyCheck→Action→MemoryUpdate | Must | REQ-007 |
| Decision Graph | 以图（非纯线性）展示决策分支（类 LangGraph） | Should | 图可渲染且节点对应 Trace |
| World Model Explorer | 展示对象 Properties/Relations/Events/Actions | Should | 点击对象可展开证据 |
| Memory Viewer | 展示 Working/Episodic/Semantic/Ontology 四类记忆 | Could | 记忆条目可查 |
| 三层视图 | Executive/Operator/Developer 分角色渲染 | Must | REQ-009 |
| WebSocket 实时流 | `/api/v1/wb/agent/{id}/stream` 推送 reasoning 等事件，断线可补齐 | Must | REQ-009 |

### 4.4 业务规则
- 每步事件必须含 `object_refs`（绑定 Ontology 对象）；缺省则采集端拒绝记录。
- Event Bus 分发同源：Trace Storage / UI Stream / Memory / Audit 消费同一 `RuntimeEvent`。
- 推理展示须呈现 Evidence（如 `PaymentLate ×3`、`Order ↓30%`），禁止展示无对象依据的裸结论。
- 实时流与持久化 `wb_trace_event` 须保证不丢审计。

### 4.5 异常与边界
| 场景 | 处理 |
|------|------|
| Agent 未运行 / 无 trace | UI 显示空态与提示，不报错 |
| WebSocket 断线 | 支持重连并补齐历史（从 `wb_trace_event` 回放） |
| 大规模 trace | Timeline 分页/虚拟滚动；Decision Graph 支持折叠 |
| 事件缺 `object_refs` | 采集端拒绝，记审计告警 |

### 4.6 原型/流程图
```
Human → Agent Observation UI → Trace API → Agent Runtime ↔ Ontology Kernel + Event Bus → Turso
```
（详细图见 `03_架构设计/agent-observation-design.md` 4.9；原型图后续补 `/docs/assets/agent-observation-ui.png`）

## 5. 风险点 & 注意事项
- PRD 与接口文档不一致会导致开发返工，需同步评审（`05_接口文档` 待补 Trace API）。
- 可解释性是差异化核心，若 `object_refs` 约束被放宽，产品定位将退化为普通日志面板。

## 6. 关联文档
- [../02_需求设计/requirement-all.md](../02_需求设计/requirement-all.md)
- [../02_需求设计/acceptance-criteria.md](../02_需求设计/acceptance-criteria.md)
- [../03_架构设计/agent-observation-design.md](../03_架构设计/agent-observation-design.md)
- [../03_架构设计/architecture-total.md](../03_架构设计/architecture-total.md)
- [../03_架构设计/pi-agent-integration.md](../03_架构设计/pi-agent-integration.md)
- [../05_接口文档/module-template-api.md](../05_接口文档/module-template-api.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成（Agent Observation Kernel PRD） |
| V1.1 | lushaohui | 2026-08-11 | 纠正：MVP 接入 PI Agent（LLM 驱动），观察层与其配合，非"不做 Agent" |
| V1.2 | lushaohui | 2026-08-11 | Pi 降级为 Cognitive Engine；观察层与 OpenAIP Agent Runtime（Planner/Policy）配合 |
| V1.3 | lushaohui | 2026-08-11 | 方案B对齐：REQ-007/008 归 V0.1 基建、REQ-009 归 V0.3 UI；TraceEvent/trace_event 模型统一 |
| V1.4 | lushaohui | 2026-08-11 | 表名对齐 table-design 约束：`trace_event` → `wb_trace_event` |
