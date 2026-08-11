# PRD：Pi 认知引擎接入（Agent Integration）
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-02-006 |
| 所属模块 | WorkBuddy-Requirement |
| 版本 | V1.1 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
把「Pi（`@earendil-works/pi-coding-agent`）接入 OpenAIP-OS 作 Cognitive Engine」拆成可开发的详细 PRD，明确边界、交互、业务规则与异常分支。本文对应 REQ-010（Pi 作 Cognitive Engine）与 REQ-011（OpenAIP Planner 自有核心），是 `pi-agent-integration.md` 的需求侧展开，驱动 `api-agent-trace.md` 与 `module-split.md` 的 `agent/pi-adapter`。

## 2. 业务/技术背景
- 来源：`03_架构设计/pi-agent-integration.md`（Pi 作 Cognitive Engine，非 Agent 本体）；`03_架构设计/architecture-total.md` 4.4/4.7 分层定位；`03_架构设计/agent-observation-design.md`。
- 关键约束：OpenAIP 拥有 Agent Runtime（Planner / Policy / Trace）、Ontology Kernel、World Model；Pi 仅负责推理 / NLU / 工具调用，经 `AgentEngine` trait 接入且可替换。
- 关联：`02_需求设计/requirement-all.md`（REQ-010/011）、`02_需求设计/prd-agent-observation.md`、`05_接口文档/api-agent-trace.md`、`09_迭代与版本/sprint-plan.md`（V0.1）。

## 3. 核心规则 & 约束（重点）
- [强制] Pi 经 `AgentEngine` trait（`PiAgentEngine`）接入，OpenAIP 不直接依赖 Pi 内部结构；未来可替换为 `ClaudeAgentEngine` / `QwenAgentEngine` / `LocalLLMEngine`。
- [强制] Pi 仅承担认知职责（推理 / NLU / 工具调用）；**禁止** Pi 直接掌管 Ontology / World Model / Policy / Trace。
- [强制] Pi 不修改 OpenAIP-OS 源码与数据文件；其工具调用须经 Capability Registry 路由到 Ontology 操作。
- [强制] Pi 每次认知输出必须映射为 `TraceEvent` 并绑定 `object_refs`，由 Observation Kernel 暴露。
- [建议] Pi 进程通过 `--mode rpc` / `--mode json` 集成；具体 RPC 协议与上下文格式以 `api-agent-trace.md` 4.6 为准。

## 4. 详细设计/内容

### 4.1 功能概述
本 PRD 在 V0.3（=MVP）落地，依赖 V0.1 内核与 V0.2 世界模型先成型。将 Pi 作为可替换认知引擎接入 OpenAIP Agent Runtime：用户输入经 Pi 完成 NLU 得到 Intent，OpenAIP 的 Planner 基于 Ontology 世界模型生成 PlanStep，Executor 经 Capability Registry 执行，全程产出 `TraceEvent` 并经 Event Bus 推送至 Observation UI。Pi 自身不持有世界状态。

### 4.2 用户角色与权限
| 角色 | 权限 | 说明 |
|------|------|------|
| Developer | 触发运行、查看全量 Trace（Developer View） | 调试 Pi 的 NLU/推理行为 |
| Operator | 触发运行、查看 Operator View（Pipeline 进度） | 运维 Agent 运行状态 |
| Executive | 只读 Executive View（Mission/Progress/Decision） | 管理者，隐藏思考细节 |
| Human-in-loop | 在 Policy 要求时审批 Action | 见 `agent-observation-design.md` 4.5 |

### 4.3 功能详情
| 功能点 | 描述 | 优先级 | 验收标准 |
|--------|------|--------|----------|
| AgentEngine 接入 | `PiAgentEngine` 实现 `AgentEngine::reason(context)->Decision` | Must | 单测断言 reason() 返回 Decision 且结构稳定 |
| 上下文注入 | 经 `AGENTS.md` / `CLAUDE.md` 注入 OpenAIP 项目指令与 Ontology 概要 | Must | Pi 启动时能读取项目指令 |
| NLU 解析 | Pi 将用户输入解析为 Intent（绑定 `object_refs`） | Must | 给定"降低客户流失"返回 Intent=Customer Retention 且 object_refs=[Customer_001] |
| Planning 路由 | OpenAIP Planner 消费 Intent，生成 PlanStep 并经 Capability Registry 执行 | Must | 链路 Goal→LLMGoalParser→Planner→PlanStep→Capability→Execute 跑通 |
| Trace 映射 | Pi 每步输出映射为 `TraceEvent`（event_type + object_refs） | Must | 每个 trace 均含 object_refs，缺失则写失败 |
| 实时流 | 运行经 WebSocket `/api/v1/wb/agent/{id}/stream` 推送 | Must | 前端实时收到 reasoning 等事件 |
| 引擎可替换 | 同一 `AgentEngine` trait 可换 `ClaudeAgentEngine` 等 | Should | 配置切换后运行不依赖 Pi 内部结构 |
| 引擎健康 | Pi 进程启停 / 超时 / 崩溃恢复由 `agent/pi-adapter` 管理 | Must | Pi 退出时运行安全失败（WB2004） |

### 4.4 业务规则
- 所有对世界模型的写操作须经 Capability Registry，杜绝 Pi 绕过 Ontology 直接落库。
- `reason()` 的 `Context` 必须包含当前 Ontology 世界模型快照（对象 / 关系 / 事件），Pi 仅基于该快照推理。
- 每次 `Decision` 必须附 `confidence`，低于阈值（默认 0.5）时标记需 Human Approval。
- `TraceEvent.object_refs` 为空视为数据异常，写入侧拦截并报 WB2006。
- Pi 进程生命周期与 OpenAIP 进程解耦；认知引擎不可用不应阻断 Ontology 基础读写。

### 4.5 异常与边界
| 场景 | 处理 |
|------|------|
| Pi 进程未启动 / 崩溃 | 触发运行返回 WB2004，运行置 Failed |
| LLM Provider 鉴权失败 | 经 `agent/pi-adapter` 捕获，返回 WB2004 并提示配置 |
| Pi 越权尝试直写文件 | Capability Registry 拒绝，记 Audit，运行失败 |
| Trace 缺 object_refs | 写入拦截 WB2006，事件不落库 |
| Policy 拒绝 Action | 返回 WB2005，等待 Human Approval |
| WebSocket 断线 | 客户端 `?after={lastEventId}` 重连补齐历史 |

### 4.6 原型/流程图
```
用户输入
   │
   ▼
[agent/pi-adapter] ──(NLU)──▶ Pi (Cognitive Engine: 推理/工具调用)
   │                               │  reason(Context)->Decision
   ▼                               ▼
OpenAIP Planner ◀── Intent ───────┘
   │  Goal→LLMGoalParser→Planner→PlanStep
   ▼
Capability Registry ──▶ Ontology Runtime ──▶ World Model
   │
   ▼
Event Bus ──▶ Trace Storage / UI Stream / Memory / Audit
   │
   ▼
Agent Observation UI (Executive/Operator/Developer View + Decision Graph)
```

## 5. 风险点 & 注意事项
- 认知引擎与 OpenAIP 的上下文传递协议需冻结，避免 Pi 越权操作文件系统。
- `reason()` 的 `Context` 体积过大可能影响 Pi 推理成本，需裁剪为最小必要快照。
- 引擎可替换性若无抽象护栏，易退化回"Pi 增强版"，须以 `AgentEngine` trait 为评审卡点。
- PRD 与 `pi-agent-integration.md` / `api-agent-trace.md` 任一变更需三方同步评审。

## 6. 关联文档
- [../03_架构设计/pi-agent-integration.md](../03_架构设计/pi-agent-integration.md)
- [../03_架构设计/agent-observation-design.md](../03_架构设计/agent-observation-design.md)
- [../03_架构设计/architecture-total.md](../03_架构设计/architecture-total.md)
- [../03_架构设计/module-split.md](../03_架构设计/module-split.md)
- [../02_需求设计/requirement-all.md](../02_需求设计/requirement-all.md)
- [../02_需求设计/prd-agent-observation.md](../02_需求设计/prd-agent-observation.md)
- [../05_接口文档/api-agent-trace.md](../05_接口文档/api-agent-trace.md)
- [../09_迭代与版本/sprint-plan.md](../09_迭代与版本/sprint-plan.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成：Pi 作 Cognitive Engine 接入 PRD（功能/角色/业务规则/异常边界/流程图） |
| V1.1 | lushaohui | 2026-08-11 | 方案B对齐：定位 V0.3（=MVP）；Trace 事件统一为 TraceEvent |
