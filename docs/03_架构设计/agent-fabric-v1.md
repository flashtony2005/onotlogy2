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
- payload 约定：`args` 数组透传为命令行参数；`task` 文本追加为末参。
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

## 修订日志

| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-16 | 契约定稿：四 ABI（AgentMessage/Session/Adapter/Router）+ Event Log 集成 + OAB v0.1 边界 |
| V1.2 | lushaohui | 2026-08-17 | T1 审批接入：fabric 挂起态（RequiresApproval → approval_requested + pending，不再 Err）；`fabric.resume`（裁决后续跑 adapter.send + 终局事件）/ `reject_event`；复用 /run/:run_id/approval 端点（ResumeTarget::FabricRun）；tc_approve_1–4 全链路（挂起→列表→approve 续跑→世界状态 / reject 终止 / 二次裁决 404） |
| V1.1 | lushaohui | 2026-08-16 | 真实适配器落地：ProcessAdapter 桥接 ProcessWorker（codex/dsh 工厂 + PATH 探测 + payload 约定）；stream 逐行进度/cancel kill 语义（tc_f6–f8 钉死） |
