# 模块拆分与依赖
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-03-002 |
| 所属模块 | WorkBuddy-Architecture |
| 版本 | V1.6 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
定义 OpenAIP-OS MVP 单体内的模块边界、职责与依赖（DAG），以及 Pi 作为可替换 Cognitive Engine 的接入方式，避免循环依赖与业务特化。

## 2. 业务/技术背景
- 依赖 `03_架构设计/architecture-total.md`（极简三原则、分层架构 4.7）、`03_架构设计/pi-agent-integration.md`（Pi 作 Cognitive Engine）。
- 目标结构（用户给定）：`runtime/`（ontology / world_model / planner / policy / trace）、`agent/pi-adapter`、`storage/turso`。

## 3. 核心规则 & 约束（重点）
- [强制] 模块依赖必须是有向无环图（DAG），禁止循环依赖。
- [强制] 每个模块单一职责；Kernel 层（ontology / world_model / storage）禁止出现业务类型特化。
- [强制] Pi 仅通过 `AgentEngine` trait 接入，`runtime/planner` 调用其 `reason()`；OpenAIP 不依赖 Pi 内部结构，未来可替换为 Claude / Qwen / LocalLLM 引擎。
- [强制] 高级能力（GraphRAG / Workflow / Evolution）只能以插件形式依赖 runtime，不得反向污染 Kernel。
- [建议] 依赖方向单向：`runtime/planner → runtime/ontology`、`runtime/planner → agent/pi-adapter`、`runtime/trace → storage`。

## 4. 详细设计/内容

### 4.1 模块树（MVP 单体，不拆微服务）
```
openAIP-os/
├── runtime/
│   ├── ontology/      # ObjectDef/Instance、IR、Compiler、ObjectEngine
│   ├── world_model/   # Relation + Event → World Model 状态聚合
│   ├── planner/       # Goal → LLMGoalParser → Planner → PlanStep → Capability → Execute
│   ├── policy/        # Action 约束与审批（Policy Check）
│   └── trace/         # TraceEvent(world/cognitive)、Event Bus、Trace API、Observation UI
├── agent/
│   └── pi-adapter/    # impl AgentEngine for Pi（Cognitive Engine）
└── storage/
    └── turso/         # LibSQLStorage（libSQL）
```

### 4.2 模块职责
| 模块 | 职责 | 对外接口 | 依赖 |
|------|------|----------|------|
| runtime/ontology | ObjectDef/Instance、IR、Compiler、ObjectEngine CRUD | `ObjectEngine` / `compile(odl)` | - |
| runtime/world_model | Relation + Event 聚合为世界模型状态 | `query_world()` | runtime/ontology |
| runtime/planner | Goal→LLMGoalParser→Planner→PlanStep→Capability→Execute | `plan(goal)` | runtime/ontology, agent/pi-adapter, runtime/policy |
| runtime/policy | Action 约束与审批 | `check(action) -> Allow/Deny` | runtime/ontology |
| runtime/trace | TraceEvent(world/cognitive) 模型、Event Bus、Trace API、Observation UI | `record(TraceEvent)` / `GET /api/v1/wb/agent/trace` / `WS /api/v1/wb/agent/{id}/stream` | runtime/ontology, storage |
| agent/pi-adapter | Pi 作 Cognitive Engine，impl `AgentEngine` | `AgentEngine::reason` | Pi 进程 |
| storage/turso | LibSQLStorage 持久化（含 `wb_ontology_object` / `wb_relation_edge` / `wb_trace_event`） | `Storage` | - |
| oaip-cli | 命令行入口 | `oaip compile / run / object list / agent run` | runtime/* |

### 4.3 AgentEngine trait（可替换认知引擎）
```rust
trait AgentEngine {
    fn reason(&self, context: Context) -> Decision;
}
```
- `PiAgentEngine`（MVP）：包装 Pi 进程（`@earendil-works/pi-coding-agent`），提供推理 / NLU / 工具调用。
- 未来实现：`ClaudeAgentEngine` / `QwenAgentEngine` / `LocalLLMEngine`。

### 4.4 依赖关系（DAG）
```
runtime/planner ──▶ runtime/ontology
runtime/planner ──▶ agent/pi-adapter      (Cognitive Engine: reason())
runtime/planner ──▶ runtime/policy
runtime/world_model ──▶ runtime/ontology
runtime/trace ──▶ runtime/ontology
runtime/trace ──▶ storage/turso
agent/pi-adapter ──▶ pi 进程
```
无回边、无循环；观察层单向依赖 runtime / storage / event bus，不反向污染 Kernel。

### 4.5 插件化 / 可替换认知引擎
```
OpenAIP Agent Runtime
      │
 ─────────────────
 PiAgentEngine        (MVP)
 ClaudeAgentEngine    (未来)
 QwenAgentEngine      (未来)
 LocalLLMEngine       (未来)
 GraphRAG Plugin      (依赖 Object Graph)
 Workflow Plugin
 Evolution Plugin
```
`AgentEngine` 多实现可热替换；OpenAIP 核心资产（Ontology / World Model / Agent Runtime / Decision Trace）不依赖具体引擎。

## 5. 风险点 & 注意事项
- 隐式依赖（如 plugin 直接耦合 storage 内部）需显式登记，保持 DAG。
- 切勿为"方便"让 planner 直接 import Pi 内部结构，须经 `AgentEngine` trait。
- Pi 进程生命周期（启动 / 超时 / 崩溃恢复）由 `agent/pi-adapter` 统一管理。

## 6. 关联文档
- [./architecture-total.md](./architecture-total.md)
- [./pi-agent-integration.md](./pi-agent-integration.md)
- [./agent-observation-design.md](./agent-observation-design.md)
- [../05_接口文档/module-template-api.md](../05_接口文档/module-template-api.md)
- [../09_迭代与版本/sprint-plan.md](../09_迭代与版本/sprint-plan.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成 |
| V1.1 | lushaohui | 2026-08-11 | 填入 crate 拆分（ontology-ir/compiler/runtime/storage/cli）、DAG 依赖、插件化架构 |
| V1.2 | lushaohui | 2026-08-11 | 新增可观察性 crate（eventbus/trace/trace-api/observation-ui）与依赖边 |
| V1.3 | lushaohui | 2026-08-11 | 新增 ontology-agent（PI Agent 集成）crate 与依赖边；Agent Plugin 标注 MVP 已接入 |
| V1.4 | lushaohui | 2026-08-11 | 重构为 runtime/agent/storage 模块树；引入 AgentEngine trait（PiAgentEngine 可替换）；Pi 降级为 Cognitive Engine |
| V1.5 | lushaohui | 2026-08-11 | 方案B对齐：trace crate 用统一 TraceEvent(world/cognitive)；存储表 agent_trace_event 泛化为 trace_event |
| V1.6 | lushaohui | 2026-08-11 | 表名对齐 table-design 约束：trace_event → wb_trace_event |
