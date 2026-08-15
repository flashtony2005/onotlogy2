# Universal Worker & Plugin Runtime（Phase 10 · V10.1 Final，设计定稿 + Implementation Spec）

| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-03-V10.1-WORKER-RUNTIME |
| 状态 | **设计定稿 V10.1 Final**（评审拍板） |
| 关系 | 在 V10.0（Plugin & Capability Runtime + Session Event Log）之上的正式升级；V10.0 文档继续有效 |
| 定位 | 只解决一个问题：**如何让 OpenAIP-OS 统一接入、发现、选择、约束和调度外部 Worker 与 Tool** |

## 0. 一句话定义

> 只要一个外部系统能够完成某种 Capability，就可以作为 **Worker** 被 OpenAIP-OS
> 注册、发现、路由、授权、执行、记录和评价。Codex / Claude Code / Gemini CLI /
> 任意 CLI / MCP Server / HTTP API / Python-Node-Shell / 企业内部 Agent
> 只是这个标准的第一批 Worker。

```
             OpenAIP-OS → Runtime → Worker
       ┌──────────┼──────────┐
       ↓          ↓          ↓
     Codex      Claude       CLI
       └──────────┼──────────┘
                  ↓
              Real World
```

## 1. 五核心 → 六核心（V10.1 正式调整）

| # | V10.0 | V10.1 |
|---|-------|-------|
| 1 | Plugin System | Plugin System（怎么接） |
| 2 | Capability Binding | Capability Registry & Binding（需要什么能力） |
| 3 | Capability Runtime | Capability Runtime（可靠执行） |
| 4 | Provider & Tool Routing | **Worker Registry & Routing（谁来完成——最大新增核心）** |
| 5 | — | **Policy & Approval（Worker 层授权，显式升为核心）** |
| 6 | Session Event Log | Session Event Log（不可变事实源） |

## 2. 六层解耦（升级）

```
Skill           怎么完成工作
  ↓
Capability      需要什么能力
  ↓
Worker Router   谁来完成
  ↓
Worker          执行主体
  ↓
Plugin          怎么接入
  ↓
Tool/AI/MCP/CLI 实际能力实现
```

**最重要的设计原则：Skill Provider Agnostic**

```
skill:
  name: fix-java-bug
  requires:
    - code.read
    - code.write
    - code.test
# 绝不写 executor: codex
```

Runtime 自己决定 Codex / Claude Code / Local Agent / Human 谁最适合——由 Router 决定。

## 3. Worker（V10.1 核心概念）

**Worker 是 Capability 的执行主体**（区别于 Provider：Provider 是资源提供方）。

```
worker:
  id: codex
  type: agent
  capabilities:
    - code.read
    - code.write
    - code.test
  input_schema: ...
  output_schema: ...
  execution:
    protocol: process
```

**四种标准接入方式（MVP）**：

| 方式 | 语义 | 示例 |
|------|------|------|
| A. Process Worker | 进程启动 | codex / claude / git / ffmpeg / python |
| B. HTTP Worker | POST /execute | 远程 Agent / 企业服务 / AI Provider / 内部服务 |
| C. MCP Worker | MCP Server 作为外部能力来源 | tools/call |
| D. Embedded Worker | Rust Worker 直接嵌入 Runtime（future） | 未来允许 |

**Provider 与 Worker 分离（架构修正）**：

```
code.write → Codex Worker → OpenAI      （Worker 是执行主体，Provider 是资源）
code.write → Claude Code Worker → Anthropic
video.edit → CLI Worker → ffmpeg
```

## 4. Worker Router

根据 `Capability + Input/Output Schema + Policy + Availability + Cost + Latency + Quality` 选择：

```
Task → Candidate Workers → Policy Filter → Score → Selected Worker
```

## 5. Policy & Approval（Worker 层，Fail Closed）

V10.0 安全设计全部保留并升级到 Worker 层：

```
Capability → Worker → Policy Check → Approval? → Execute
code.read        → allow
database.delete  → require_approval（复用 Kernel 审批挂起点）
未知/无权限      → deny（Fail Closed：没有权限 = DENY，不是 TRY）
```

## 6. Session Event Log（扩展为 Runtime-visible ⇒ Logged）

V10.0 `Model-visible ⇒ Logged` 扩展为 **Runtime-visible ⇒ Logged**：

```
goal.created / skill.selected / capability.required
worker.selected { worker, capability, reason }
policy.checked / approval.requested / approval.granted
plugin.call / plugin.result
worker.started / worker.completed / worker.failed
task.completed
```

保持 V10.0：Append Only / Immutable / Replayable / Forkable / Auditable——
为 Evaluation / Skill Mining / Skill Evolution 提供事实基础。

**Secret ≠ Event Log**：日志只记录 `credential_used: true`，绝不记录 `token=sk-xxxx`。

## 7. 与现有 V10.0 实现的映射（差距清单）

| # | 差距 | 内容 | 复用 |
|---|------|------|------|
| G1 | WorkerManifest Schema | worker: id/type/capabilities/schema/execution{protocol}（可序列化） | PluginManifest 模式 |
| G2 | WorkerRegistry | 注册/发现/状态/health | PluginRegistry 模式 |
| G3 | WorkerAdapter SDK | trait：capabilities() / execute() / health() | Plugin/AsyncPlugin |
| G4 | Process Worker（Generic CLI） | spawn 进程执行（真实）——Codex/Claude/git/ffmpeg 壳 | agent 模块 child process 先例 |
| G5 | HTTP Worker | POST /execute | HttpPlugin |
| G6 | MCP Worker | MCP server 作为 worker | McpClient |
| G7 | Codex/Claude Worker Adapter | 声明式适配器壳（协议就位；真实接入可配置） | G4 之上 |
| G8 | WorkerRouter | 候选→Policy Filter→Score→Selected（deterministic） | Router 扩展 |
| G9 | Policy/Approval 集成 | worker 级 allow/require_approval/deny + fail-closed | compose::Policy + Kernel approval |
| G10 | Session Log 集成 | worker.selected/started/completed/failed 事件 | SessionLog |
| G11 | Execution Protocol + Result Schema | worker 调用/结果契约 | EffectResult 模式 |
| G12 | Health/Status | worker 可用性探测 | — |
| G13 | Dashboard | worker 视图（注册表/路由/事件） | V2 面板模式 |

## 8. 真实世界验证（R1-R10，不做 Mock-only Runtime）

| 测试 | 验证 |
|------|------|
| R1 | Plugin → HTTP（V10.0 保留） |
| R2 | Plugin → MCP（保留） |
| R3 | Plugin → AI（保留） |
| R4 | **Plugin → CLI**（新增：Generic CLI Worker 真实进程执行） |
| R5 | Worker → Codex（适配器壳 + 真实 CLI 若可用） |
| R6 | Worker → Claude Code（适配器壳） |
| R7 | Worker → Generic CLI（git/echo 真实执行） |
| R8 | Worker → Policy / Approval（allow / require_approval / deny） |
| R9 | Worker → Event Log（worker.* 全事件链） |
| R10 | Worker → Failure / Retry（失败捕获 + 重试一次） |

## 9. 完整实例（验收场景）

> 用户：分析当前 Java 项目，修复订单查询 Bug，运行测试并生成报告。

```
Goal → Planner → Skill(Java Bug Fix)
  requires: code.read / code.write / code.test / report.generate

Worker Registry:
  code.read      → Codex
  code.write     → Codex / Claude Code
  code.test      → Maven CLI
  report.generate → Report Worker

Runtime: T1→Codex T2→Codex T3→Maven T4→Report
所有事件 → Session Event Log → Task → Evidence → Result → Report
```

## 10. 与 Phase 9 的真正接口（第三方 Skill 完整闭环）

```
第三方 Skill → AI Parser → Skill IR → Capability Extraction → Skill Registry
  → Phase 10 → Capability → Worker Registry → Codex / Claude / CLI / MCP / API
```

第三方 Skill 不需要知道 OpenAIP-OS 使用什么 AI——只需要声明：**我需要什么能力**。

## 11. Non-goals（正式写入）

- ❌ 不实现具体 AI Agent / Coding Agent / Computer Use / Browser Use
- ❌ 不实现 OS-level Computer History / activity recorder
- ❌ 不实现具体 LLM Provider
- ❌ 不绑定 Codex / Claude Code（他们是 Worker，不是依赖）
- ❌ 不把 Skill 与 Worker 绑定（Skill Provider Agnostic）

**原因**：Codex / Claude Code 等已经是优秀的 Worker——OpenAIP-OS 的价值不是重复造轮子。

## 12. 交付物（15 项）

```
1. Worker Manifest Schema      2. Worker Registry         3. Worker Adapter SDK
4. Generic CLI Worker          5. HTTP Worker             6. MCP Worker
7. Codex Worker Adapter        8. Claude Code Adapter     9. Worker Router
10. Policy/Approval 集成        11. Session Log 集成       12. Worker Execution Protocol
13. Worker Result Schema       14. Worker Health/Status   15. V10 Dashboard（Worker 视图）
```

## 13. 路线衔接

```
Phase 9  Skill Intelligence    第三方 Skill → AI 解析 → IR → Capability → Registry
Phase 10 Universal Worker Runtime  Skill → Capability → Worker Router → Codex/Claude/CLI/MCP/API → 执行 → Evidence → Event Log
Phase 11 Domain Agent Evolution Runtime  Event Log → Evaluation → Experience → Mutation → Benchmark → Promotion → Skill/Context/Router → Next Run
Phase 12 Software Engineering Ontology  Requirement → Project → Code → Test → Issue → Change → Review → Release（第一个领域实例）
```

Phase 11（见 `domain-agent-evolution-v11.md`）做 Agent Evolution 时，Codex 和 Claude Code 已经是现成的 Worker，
可直接被 Ontology Runtime 调度；Software Engineering Ontology 降为第一个领域实例（Phase 12）。

## 14. 实现规划

| 步骤 | 内容 | 文件 |
|------|------|------|
| 1 | WorkerManifest + Worker trait（SDK）+ 4 接入方式类型 | `src/worker/{mod,manifest,sdk}.rs` |
| 2 | WorkerRegistry（注册/发现/health）+ WorkerRouter（候选→Policy→Score） | `src/worker/{registry,router}.rs` |
| 3 | Process Worker（Generic CLI 真实 spawn）+ HTTP Worker + MCP Worker | `src/worker/{process,http,mcp}.rs` |
| 4 | Codex/Claude 适配器壳（G7） | `src/worker/adapters.rs` |
| 5 | Policy/Approval 集成（allow/require_approval/deny fail-closed） | `src/worker/policy.rs` |
| 6 | Session Log 集成（worker.* 事件） | `src/worker/exec.rs` |
| 7 | R4-R10 真实验证测试 + Dashboard Worker 视图 | `tests/worker_v2.rs` `ui/` |
| 8 | 门禁（fmt/clippy/test 全绿）+ 提交 | — |
