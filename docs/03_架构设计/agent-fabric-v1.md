# Agent Communication Fabric（OAB）契约 V1.0

> 定位：OpenAIP **不是** DSH ↔ Codex 的转发器，而是让不同 Agent 在统一的
> Session / Context / Capability / Policy / Event 语义下通信的 **Agent
> Communication Fabric**。
>
> 一句话：OpenAIP = Agent Communication + World Model + Policy + Execution Runtime。
> 其中 Adapter 负责协议差异，Bridge（Fabric）负责语义。
>
> 四层架构（固定）：
>
> ```
> ┌─────────────────────────────────┐
> │           Agent Layer           │  DSH / Codex / Claude / Gemini / ...
> ├─────────────────────────────────┤
> │       Communication Layer       │  AgentMessage / Session（本契约）
> ├─────────────────────────────────┤
> │         OpenAIP Runtime         │  Router / Policy / Capability / Planner / Context / Trust
> ├─────────────────────────────────┤
> │           World Layer           │  Event Log / Projection / Kernel（Phase 5 已立）
> └─────────────────────────────────┘
> ```
>
> 协议变化（DSH v1/v2、Codex v1/v2…）只影响 Adapter；OpenAIP Runtime 不变。

## 0. 第一架构原则：不直接传 DSH/Codex 的消息

```
DSH Message → Normalize → AgentMessage → Route → AgentMessage → Codex Adapter → Codex Message
```

禁止 `DSH Message → Codex Message` 直通。所有跨 Agent 通信必须经统一
`AgentMessage` 规范化——协议差异止于 Adapter。

## 1. ABI-1 AgentMessage（统一消息）

```rust
pub struct AgentMessage {
    pub id: String,                  // 消息唯一 id（new_trace_id 同源，单调）
    pub session_id: String,          // 所属会话（v0.1 必填）
    pub sender: String,              // 发送方 agent id（"dsh" / "codex" / ...）
    pub receiver: String,            // 接收方 agent id；"*" = 广播
    pub kind: MessageKind,           // 消息类型（见下）
    pub intent: Option<String>,      // 语义意图（"modify_code"）；kind=task.* 时必填
    pub payload: Value,              // 协议无关载荷（任务描述/结果）
    pub context: Value,              // 轻量上下文引用（session 内增量；全量上下文走 Session+World）
    pub capability: Option<String>,  // 请求的能力（"coding.modify"）
    pub correlation_id: Option<String>, // 关联（task.request ↔ task.result）
    pub causation_id: Option<String>,   // 因果链（前驱消息 id；事件溯源用）
    pub timestamp: i64,              // Kernel 秒轴（与 Projection normalize_ts 一致）
}
```

**消息类型（OAB v0.1 只实现 4 类）**：

| kind | 序列化 | 语义 |
|------|--------|------|
| TaskRequest | `task.request` | 发起任务（intent + capability + payload） |
| TaskAccepted | `task.accepted` | 接收方受理（correlation_id 回指） |
| TaskProgress | `task.progress` | 流式进度（payload={progress, note}） |
| TaskResult | `task.result` | 终局结果（payload={status: completed/failed, ...}） |

示例：

```json
{
  "type": "task.request",
  "sender": "dsh",
  "receiver": "codex",
  "intent": "modify_code",
  "capability": "coding.modify",
  "context": {
    "workspace": { "root": "/abs/project", "mode": "isolated" },
    "timeout_ms": 30000
  },
  "payload": { "repository": "demo", "task": "修复订单计算错误" }
}
```

## 2. ABI-2 Session（通信的第二个核心）

DSH 与 Codex 之间不是 request/response，而是 **Session**：

```rust
pub struct Session {
    pub id: String,
    pub participants: Vec<String>,   // 参与方 agent id
    pub state: SessionState,         // Active / Paused / Completed / Failed / Cancelled
    pub created_at: i64,
    pub policy_ref: Option<String>,  // 会话级策略引用（v0.1 保留，接 compose::Policy）
    messages: Vec<AgentMessage>,     // 会话消息（可经 SessionLog 事件溯源）
}

pub struct SessionManager { /* id → Session；create/open/pause/resume/close */ }
```

生命周期（状态机）：

```
Created → Active ⇄ Paused → Completed
                          ↘ Failed
                          ↘ Cancelled
```

Session 支撑：streaming（TaskProgress 累积）、cancel（会话终局）、retry /
resume（重开 Active）、timeout、human approval（policy 挂起）。

**Effective Context 原则**：消息不携带全部历史——`Message + Session + World
State → Effective Context`。DSH 说"继续刚才那个订单修复任务"，只需
`session_id`；上下文由 Session.messages + World Model（Phase 5 投影）恢复。

## 3. ABI-3 Adapter（协议差异隔离）

```rust
pub trait AgentAdapter: Send + Sync {
    fn id(&self) -> &str;
    fn connect(&self) -> Result<(), FabricError>;
    fn discover(&self) -> Vec<AgentCapability>;   // 能力声明（能力市场身份证）
    fn send(&self, msg: &AgentMessage) -> Result<AgentMessage, FabricError>; // 同步往返
    fn stream(&self, msg: &AgentMessage, tx: ...) -> Result<(), FabricError>; // 流式
    fn cancel(&self, correlation_id: &str) -> Result<(), FabricError>;
}
```

- **Adapter 管协议**（把 AgentMessage 翻译成 DSH/Codex 原语），**Worker 是执行
  连接器**：`CodexAdapter → CodexWorker（ProcessWorker）→ Codex`。
- 现有 Worker 层不推倒：`WorkerManifest` 的能力声明 = Adapter 的 discover 数据源。
- 每个 Agent 是 **Capability Provider**，不是 Bridge 的特殊公民。

**真实适配器（v1.1 落地：`fabric/adapters.rs`）**：

- `ProcessAdapter`：桥接 `ProcessWorker` 作为执行连接器（manifest/command 复用），
  执行 = 同步 std::process（阻塞上下文约定：spawn_blocking / block_in_place，
  与 StorageSink 同约定，不依赖 async runtime）。
- `codex_adapter` / `dsh_adapter`：工厂函数（能力市场与 `codex_worker` /
  `dsh_worker` 同源——Codex/DSH 是 Capability Provider，新 Agent 接入 = 一个
  适配器工厂）。
- `connect()` = 命令 PATH 探测（不启动进程；不可用 → fail-closed）。
- payload 约定：`args` 数组透传为命令行参数；`task` 文本追加为末参。执行上下文统一放在 AgentMessage `context`（Facade 请求的 `executionContext`）中；`agent.task` / `code.write` 缺少 isolated workspace 时 fail-closed。
- `stream()` 真实语义：逐行 stdout → `TaskProgress` 序列 → 终局 `TaskResult`
  （退出码非 0 → status=failed）。
- `cancel(correlation_id)`：kill 运行中的子进程（未在跑 = 幂等 Ok；
  被取消的进程不得报 completed——kill 语义）。

## 4. ABI-4 Router（Bridge 的真正核心）

不是 `receiver = codex` 直路由，而是：

```
Intent → Capability → Policy → Agent Selection
```

```rust
pub struct FabricRouter {
    intents: HashMap<String, String>,            // "modify_code" → "coding.modify"
    registry: HashMap<String, Vec<AgentOffer>>,  // capability → 提供方（priority 序）
    policy: FabricPolicy,                        // capability → allow / require_approval / deny
}

pub struct RoutePlan {
    pub agent_id: String,      // 选中 Agent（Capability Provider）
    pub capability: String,    // 解析出的能力
    pub verdict: RouteVerdict, // Allow / RequiresApproval / Denied(reason)
}
```

- 出现 Claude/Gemini/Local Coding Agent 时，Router 动态选择（registry 增删即可）。
- **Policy 位于 Router 前面**：`shell.execute` Risk=HIGH → require_approval →
  挂起 → 人工裁决（复用现有 Approval / Gate / Audit 机制）→ 放行才路由。

## 5. Event Log 是通信层的黑匣子（接 Phase 5）

所有重要通信产生事件（event_type 前缀 `fabric.`，kind=Cognitive，
run_id=None——事实不依赖 Run，与 P5 一致）：

| 事件 | 时机 |
|------|------|
| `fabric.agent_connected` / `fabric.agent_disconnected` | Adapter connect/disconnect |
| `fabric.message_sent` / `fabric.message_received` | 消息经 Router 转发 |
| `fabric.task_requested` / `fabric.task_accepted` / `fabric.task_completed` / `fabric.task_failed` | 任务生命周期 |
| `fabric.capability_invoked` | 能力被路由选中 |
| `fabric.approval_requested` / `fabric.approval_approved` / `fabric.approval_rejected` | 策略挂起裁决 |

```
Event Log → Projection → World State（Phase 5 单向流原样接续）
```

## 6. OAB v0.1 MVP 边界（本阶段只做这些）

- **5 个核心接口**：`connect() / discover() / send() / stream() / cancel()`
- **4 类消息**：task.request / task.accepted / task.progress / task.result
- **2 个关联字段**：correlation_id / session_id
- 不做完整 A2A / MCP / 复杂协议兼容；Adapter 骨架 + Fake/真实 Codex 适配器验证。
- 不动现有 Worker 层（Worker 仍是执行连接器；Fabric 是语义连接层）。

## 7. 验收（第一条真正的 Demo）

```
DSH: "让 Codex 修改 OrderService，修复金额计算问题，然后运行测试"
  → Intent=modify_code → Capability=coding.modify → Policy → Router → codex
Codex: 修改代码 → 运行测试 → 返回结果
Bridge: Codex Result → fabric.task_completed 事件 → Projection → World State
DSH: "Codex 已完成修改，4 个文件发生变化，测试全部通过"
```

契约测试钉：tc_f1 消息规范化 / tc_f2 Router 意图→能力→策略→选择 /
tc_f3 Session 生命周期 / tc_f4 事件完整性（全量入 Event Log）/
tc_f5 闭环 demo（Router→Adapter→result→事件→投影→World State）。

## 8. DSH 生产化约束（Phase 2.1）

DSH 仍通过通用 ProcessAdapter 接入，但生产执行增加三项硬约束：

1. **环境边界**：isolated workspace 下只允许 PATH、HOME、DSH_HOME、DEEPSEEK_API_KEY、NO_COLOR、TERM、LANG、LC_ALL。未知变量直接 fail-closed，避免宿主凭据和运行时状态隐式进入 DSH。
2. **结果可观测**：终局 task.result 保留原始 stdout/stderr/exit_code，并增加 duration_ms、progress_count、failure_category 与 structured_output。结构化输出只在 stdout 去空白后是合法 JSON 时标记为 json；否则标记为 text；空输出标记为 empty。
3. **审计同构**：fabric.task_completed/failed/cancelled 事件写入上述执行指标，保证 UI、CLI 和恢复流程不需要重新解析进程输出。

失败分类至少区分 execution_context、spawn_error、process_exit、timeout 和 cancelled。DSH 任务默认不自动重试：agent.task 与 code.write 可能产生副作用，重试必须由上层显式发起新的 correlation_id，并留下新的审计链。


## 9. DSH 任务协议与结果消费（Phase 2.2）

Phase 2.2 将 DSH 的执行输入和终局结果从“可执行字符串 + 原始输出”提升为可审计的任务协议与结果摘要，但协议差异仍止于 `ProcessAdapter`，Bridge 不直接理解 DSH CLI 参数。

### 9.1 结构化任务输入

当任务需要表达任务语义和验收条件时，调用方使用 `task_spec`：

```json
{
  "task_spec": {
    "description": "修复订单金额计算",
    "kind": "code_change",
    "acceptance": { "run_tests": true }
  }
}
```

- `description` 与 `kind` 必填且不能为空；`acceptance`（可选）必须是对象。
- `task` 与 `task_spec` 互斥；`args` 若存在必须是字符串数组。
- `task_spec` 在 adapter 入口处校验并规范化；非法协议在进程 spawn 前 fail-closed，并以 `protocol_error` 记录，不把协议错误伪装成 `spawn_error`。
- `task_spec.description` 是传给 DSH 的任务文本；结构化字段不会拼接进 shell 命令，也不会绕过既有 workspace 与环境策略。
- 现阶段纯文本 `task` 和显式 `args` 仍是合法执行入口；不新增兼容层，后续调用方应优先迁移到 `task_spec`。

### 9.2 Workspace 变更摘要

`ProcessAdapter` 在执行前后对正式 workspace 做文件快照：相对路径 + SHA-256 内容摘要。终局结果只返回路径和计数，不返回文件内容：

```json
{
  "workspace_diff": {
    "available": true,
    "added": ["added.txt"],
    "modified": ["changed.txt"],
    "deleted": ["deleted.txt"],
    "counts": { "added": 1, "modified": 1, "deleted": 1 }
  }
}
```

`.git`、`target`、`.oaip`、`.tmp`、`tmp` 等生成目录不参与摘要。宿主共享临时目录不作为可审计 workspace；正式 isolated workspace 才返回 `available=true`。快照失败或上下文不可用时返回 `available=false`，不能把不完整扫描误报为“无变更”。

### 9.3 测试结果摘要与消费边界

Adapter 从 DSH stdout 中识别 JSON `test_summary` 或常见测试文本（包括 Rust 的 `test result`），统一生成 `detected/passed/failed/skipped/total/status`。无法识别时只返回 `{ "detected": false }`，原始 stdout 仍保留在 task.result，避免摘要层丢失诊断信息。

`task.result.payload`、`fabric.task_completed/failed/cancelled` 审计输入和 UI 共用同一组字段：`task_spec`、`workspace_diff`、`test_summary`、`duration_ms`、`exit_code`、`progress_count`、`structured_output`、`failure_category`。因此 UI 展示和恢复流程直接消费结构化结果，不需要再次解析 DSH 输出；workspace diff 也不会把源码内容写入事件日志。

Phase 2.2 不自动重试任务、不推断未识别的测试结果，也不把 workspace diff 当作补丁应用机制。后续可在此摘要协议之上增加验收策略和可重放的显式变更记录。

## 修订日志

| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.5 | lushaohui | 2026-08-18 | Phase 2.1 DSH 生产化：环境白名单、结构化输出、执行指标与终局审计、超时/取消契约测试；默认不自动重试 |
| V1.6 | lushaohui | 2026-08-18 | Phase 2.2 DSH 任务协议与结果消费：task_spec、workspace_diff、test_summary、protocol_error 分类及 UI/审计同构 |
| V1.0 | lushaohui | 2026-08-16 | 契约定稿：四 ABI（AgentMessage/Session/Adapter/Router）+ Event Log 集成 + OAB v0.1 边界 |
| V1.2 | lushaohui | 2026-08-17 | T1 审批接入：fabric 挂起态（RequiresApproval → approval_requested + pending，不再 Err）；`fabric.resume`（裁决后续跑 adapter.send + 终局事件）/ `reject_event`；复用 /run/:run_id/approval 端点（ResumeTarget::FabricRun）；tc_approve_1–4 全链路（挂起→列表→approve 续跑→世界状态 / reject 终止 / 二次裁决 404） |
| V1.1 | lushaohui | 2026-08-16 | 真实适配器落地：ProcessAdapter 桥接 ProcessWorker（codex/dsh 工厂 + PATH 探测 + payload 约定）；stream 逐行进度/cancel kill 语义（tc_f6–f8 钉死） |
