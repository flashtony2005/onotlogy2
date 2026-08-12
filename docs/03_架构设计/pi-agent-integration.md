# Pi Agent 集成设计（PI Agent Integration）
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-03-006 |
| 所属模块 | WorkBuddy-Architecture |
| 版本 | V1.3 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
定义 Pi（`@earendil-works/pi-coding-agent`，中文文档见 https://pi-doc.com/docs/latest/quickstart）在 OpenAIP-OS 中的定位与接入方式。核心结论：**Pi 不是 OpenAIP-OS 的 Agent 本体，而是其 Cognitive Engine（认知引擎）**——通过 `AgentEngine` trait 提供的可替换 LLM 执行引擎，负责推理、自然语言理解（NLU）与工具调用；OpenAIP-OS 自身拥有 Agent Runtime（Planner / Policy / Trace）、Ontology Kernel 与 World Model。本文是 `agent-observation-design.md` 中"Agent 接入"的具体落地规格，并与 `module-split.md` 4.3 的 `AgentEngine` 定义一致。

## 2. 业务/技术背景
- 来源：Pi 官方 Quickstart（pi-doc.com）。Pi 为 Earendil Inc. 出品的编码 Agent，以 npm 全局包 `@earendil-works/pi-coding-agent` 分发；LLM 能力来自 Qwen / Claude / GPT 等 Provider（API Key 或订阅登录），支持 `--mode rpc` / `--mode json` 进程集成。
- 设计原则：复用成熟 Agent 作认知引擎，不自研 LLM 推理（符合优先使用成熟依赖）；OpenAIP 自有资产是 Ontology + World Model + Agent Runtime + Decision Trace。
- 关联：`03_架构设计/architecture-total.md`（4.4 能力边界、4.6/4.7 分层）、`03_架构设计/module-split.md`（runtime/agent/storage、AgentEngine trait）、`02_需求设计/requirement-all.md`（REQ-010/011）、`02_需求设计/prd-agent-observation.md`。

## 3. 核心规则 & 约束（重点）
- [强制] Pi 以 `AgentEngine` trait 实现接入（`PiAgentEngine`），OpenAIP 不直接依赖 Pi 内部结构；未来可替换为 `ClaudeAgentEngine` / `QwenAgentEngine` / `LocalLLMEngine`。
- [强制] Pi 仅承担认知职责：推理、NLU、工具调用；**禁止**让 Pi 直接掌管 Ontology / World Model / Policy / Trace，这些由 OpenAIP Agent Runtime 拥有。
- [强制] Pi 不修改 OpenAIP-OS 源码与数据文件；其工具调用须经 OpenAIP 的 Capability Registry 路由到 Ontology 操作。
- [强制] Pi 的每一步认知输出须映射为 `TraceEvent`（绑定 `object_refs`），由 Observation Kernel 暴露。
- [建议] Pi 进程通过 `--mode rpc` / `--mode json` 集成；具体 RPC 协议与上下文传递格式见 `05_接口文档/api-agent-trace.md` 4.6（WebSocket 流与 `TraceEvent` DTO 已冻结）。

## 4. 详细设计/内容

### 4.1 分层定位
```
Human → Agent UI → Decision Trace UI
        ↓
================================
     OpenAIP Agent Runtime
   [Planner | Policy | Ontology Runtime
    | World Model | Event Bus | Capability Registry]
        ↓ (调用)
================================
        pi agent  (Cognitive Engine)
     LLM Agent Execution Engine
        ↓
================================
       Qwen / Claude / GPT
```

### 4.2 AgentEngine trait
```rust
trait AgentEngine {
    fn reason(&self, context: Context) -> Decision;
}
```
- `PiAgentEngine`（MVP）：包装 Pi 进程，提供推理 / NLU / 工具调用。
- 未来实现：`ClaudeAgentEngine` / `QwenAgentEngine` / `LocalLLMEngine`。

### 4.3 V0.3（=MVP）执行流
```
用户输入 → pi agent (NLU) → Intent → Ontology Query
        → Planner → Action → Executor → Trace UI
```
该闭环在 V0.3（=MVP）落地，依赖 V0.1 内核与 V0.2 世界模型先成型。OpenAIP 负责 Ontology Query / Planner / Executor / Trace；Pi 负责 NLU 与推理。Planner 已是 OpenAIP 接近未来企业 Agent 的核心（Goal→LLMGoalParser→Planner→PlanStep→Capability→Execute），不应建成"Pi 增强版"。

### 4.4 认证与配置
Pi 的 LLM Provider 认证（API Key / 订阅）由运行环境提供，OpenAIP 不内置模型权重；Pi 启动上下文通过 `AGENTS.md` / `CLAUDE.md` 注入项目指令。

`PiAgentEngine` 以 `--mode text --print --no-session --no-tools` 非交互调用 Pi（stdout 直接为模型文本，约定其返回 JSON 并由 `parse_decision` 解析）。LLM Provider 选择：
- 不设置时回退到 Pi 内置默认 provider（`~/.pi/agent/settings.json` 的 `defaultProvider`）。
- 可通过环境变量覆盖（便于接入本地网关，不改动代码）：`OPENAIP_PROVIDER` / `OPENAIP_MODEL` / `OPENAIP_API_KEY_ENV`。

### 4.6 接入本地 OpenAI 兼容网关（如 cc switch）
Pi 支持在 `~/.pi/agent/models.json` 中注册自定义 OpenAI 兼容 provider。以本地网关 `http://127.0.0.1:5000`（路径 `/v1`，免鉴权，模型 slug 如 `deepseek-v4-flash`）为例：

```json
{
  "providers": {
    "ccswitch": {
      "name": "CC Switch (local OpenAI gateway @127.0.0.1:5000)",
      "api": "openai-completions",
      "baseUrl": "http://127.0.0.1:5000/v1",
      "apiKey": "sk-noauth",
      "models": [
        { "id": "deepseek-v4-flash", "name": "Deepseek-v4-flash" }
      ]
    }
  }
}
```
> 注意：该 pi 版本注册的 api 名为 `openai-completions` / `openai-responses` / `openai-codex` / `anthropic` / `google` / `mistral` 等，**没有裸 `openai`**；标准 `/v1/chat/completions` 网关用 `openai-completions`。

启动 OpenAIP 时指定该 provider 即可走本地网关：
```bash
OPENAIP_PROVIDER=ccswitch OPENAIP_MODEL=deepseek-v4-flash \
  cargo run --bin oaip -- serve
# 或 CLI 触发：OPENAIP_PROVIDER=ccswitch OPENAIP_MODEL=deepseek-v4-flash ./target/debug/oaip agent run "reduce churn"
```
验证：网关 `/v1/models` 返回 200（OpenAI 格式）即兼容；`oaip agent run` 跑通 8 步认知闭环即接入成功。

### 4.5 为什么这样切分
OpenAIP 的核心资产是 **Ontology Kernel + World Model + Agent Runtime + Decision Trace**——这是 Palantir AIP 真正难复制的部分；Pi 是可替换的认知引擎，避免把 OpenAIP-OS 降级为"Pi 增强版"。

## 5. 风险点 & 注意事项
- Pi 进程生命周期管理（启动 / 超时 / 崩溃恢复）由 `agent/pi-adapter` 统一负责。
- 认知引擎与 OpenAIP 之间的上下文传递协议需冻结，避免 Pi 越权直接操作文件系统。
- 所有对世界的写操作必须经 Capability Registry，杜绝 Pi 绕过 Ontology 直接落库。

## 6. 关联文档
- [./architecture-total.md](./architecture-total.md)
- [./module-split.md](./module-split.md)
- [./agent-observation-design.md](./agent-observation-design.md)
- [../02_需求设计/requirement-all.md](../02_需求设计/requirement-all.md)
- [../02_需求设计/prd-agent-integration.md](../02_需求设计/prd-agent-integration.md)
- [../05_接口文档/api-agent-trace.md](../05_接口文档/api-agent-trace.md)
- [../02_需求设计/prd-agent-observation.md](../02_需求设计/prd-agent-observation.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成：Pi 作 Cognitive Engine（AgentEngine trait），非 Agent 本体；分层定位、MVP 执行流、核心资产切分 |
| V1.1 | lushaohui | 2026-08-11 | 待确认项指向 api-agent-trace.md 4.6；补充 PRD 与接口交叉引用 |
| V1.2 | lushaohui | 2026-08-11 | 方案B对齐：4.3 执行流定位 V0.3（=MVP），依赖 V0.1 内核 + V0.2 世界模型 |
| V1.3 | lushaohui | 2026-08-12 | 4.4 补充 `OPENAIP_PROVIDER`/`OPENAIP_MODEL` 环境变量覆盖；新增 4.6 接入本地 OpenAI 兼容网关（cc switch @127.0.0.1:5000，`api: openai-completions`） |
