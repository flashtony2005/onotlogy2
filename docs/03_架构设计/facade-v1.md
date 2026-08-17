# OpenAIP Public Semantic Facade 契约 V1.0

> 定位：**OpenAIP 的公共语义 API**——不是「DSH Plugin 的前置工作」。
> Facade 稳定后，DSH Plugin / CLI / HTTP Server / 未来 MCP / A2A 都只是不同的
> Transport / Adapter。
>
> 核心边界第一次清晰化：
>
> ```
> Kernel 是内部世界；Facade 是语义 API；Plugin / Adapter 是外部连接层
> ```

## 0. 双向模型

同一个 DSH 可以同时处于两个位置：

```
DSH Plugin 是 OpenAIP 的入口（调用 OpenAIP）。
DSH Adapter 是 OpenAIP 的出口（被 OpenAIP 调用）。
```

```
DSH
 │
 ├────→ OpenAIP Plugin        （入口：调 OpenAIP Facade——world/audit/fabric）
 │
 └────← OpenAIP dsh_adapter   （出口：fabric 的一等 Agent，被 Router 调度）
```

因此 OpenAIP 成为 DSH 与 Codex 的通讯桥接——不是转发器，是语义层。

## 1. 架构

```
                    OpenAIP
                       │
                 ┌─────┴─────┐
                 │   Facade  │
                 └─────┬─────┘
                       │
              ┌────────┼────────┐
              │        │        │
           world     audit    fabric
           .query    .query   .submit
              │        │        │
              │        │        ▼
              │        │      Router
              │        │        │
              │        │   ┌────┴────┐
              │        │   ▼         ▼
              │        │  DSH      Codex
              │        │ Adapter  Adapter
              │        │
              └────────┴───────────────
                       │
                  Event / Kernel
```

## 2. 内部不暴露原则

对外只认三个语义域——**不直接暴露**：

```
kernel.query()      event_log.query()     storage.query()
router.execute()    worker.execute()
```

这些全部是内部实现。对外只有：

```
OpenAIP Public Semantic API
  world.query()     —— 世界状态（Event Log → Projection → Kernel 的只读视图）
  audit.query()     —— 审计（Event Log 的审计语义域：approval.* / evolution.gate_approved / fabric.* / policy.denied）
  fabric.submit()   —— 经 Router 向 Capability Provider 提交任务（双向模型出口）
```

## 3. 最小 Rust API（本契约 v1.0 只实现这三个）

```rust
pub trait OpenAipFacade {
    fn world_query(&self, request: WorldQuery) -> Result<WorldView, FacadeError>;
    fn audit_query(&self, request: AuditQuery) -> Result<AuditView, FacadeError>;
    fn fabric_submit(&self, request: FabricRequest) -> Result<FabricSubmission, FacadeError>;
}
```

**类型定义**：

| 类型 | 字段 | 语义 |
|------|------|------|
| `WorldQuery` | `object_ref: Option<String>`、`predicate: Option<String>`、`limit: usize` | 按实体/谓词过滤；默认全量 |
| `WorldView` | `relations: Vec<WorldRelation>`、`total: usize`、`at: i64` | 世界关系（source/predicate/target/valid_from/valid_until）；`at` = 视图时刻（Kernel 秒轴） |
| `AuditQuery` | `run_id/agent_id/event_type: Option<String>`、`limit: usize` | event_type 前缀匹配（"approval." 匹配全部 approval.*） |
| `AuditView` | `events: Vec<AuditEvent>`、`total: usize` | 审计事件（id/event_type/refs/payload/timestamp） |
| `FabricRequest` | `session_id/sender/intent: String`、`payload: Value` | 提交任务；session 不存在自动创建（participants=[sender]） |
| `FabricSubmission` | `result: AgentMessage` | task.result（status=completed/failed + payload） |

**内部服务**：

```
OpenAipFacade
      │
      ├── WorldService   （每次查询重建投影：per-query 全量，P5.4 策略；
      │                    P3 watermark 常驻 Kernel 属后续里程碑——性能问题非正确性问题）
      ├── AuditService   （直读 Event Log 审计域）
      └── FabricService  （Bridge.submit + SessionManager；事件经 P5.6 同步 sink 落库）
```

**执行约定**：Facade 是同步 API；内部以 `tokio::runtime::Handle::block_on` 驱动
storage（阻塞上下文约定：server=spawn_blocking、CLI=block_in_place、插件=宿主线程）。

**单一语义源**：CLI（`oaip facade world query` / `audit query` / `fabric submit`）、
Server（HTTP /api/world/query 等）、Plugin 全部最终调用 `OpenAipFacade`——
不允许 CLI 一套语义、Server 一套语义、Plugin 又一套语义。

**CLI Transport 形态（v1.1 已落地）**：每次命令独立进程——Facade 重新装配
（storage 打开 + 适配器 connect + Router 注册），因此每次命令产生一轮
`fabric.agent_connected` 事件（审计可见，属进程模型的预期行为）。
意图映射以 WorkerManifest 能力市场为准（如 `modify_code → code.write`，
codex 声明的是 code.write 系而非契约示例的 coding.modify——能力名由
Adapter discover 决定，语义由意图映射层对齐）。

## 4. 扩展方法

原列于契约「未来扩展」的四个方法已在 `fix/survivor-gaps` 落地为 Facade API
（`FabricService` 实现，`Facade` 以固有方法暴露，与 `fabric_resolve` 同约定）：

- `fabric.cancel(correlation_id)` → `Facade::fabric_cancel`：按 correlation_id 取消运行
  （反查所属 session 标记 Cancelled + 通知各 adapter，best-effort / idempotent）。
- `fabric.stream(session_id)` → `Facade::fabric_stream`：返回会话消息快照（同步；
  实时推送由 `/api/v1/wb/agent/:id/stream` WS 端点承担）。
- `capability.discover()` → `Facade::capability_discover`：已接入能力提供者（adapter）id 列表。
- `session.get(session_id)` → `Facade::session_get`：按 id 取会话副本（participants/state/messages）。

仍属未来扩展（未实现）：

```
world.observe() / world.subscribe()
audit.trace()
```

## 5. 验收（契约测试）

- tc_g1 world_query：事件 → 投影 → 世界视图（无过滤 / object_ref / predicate）
- tc_g2 audit_query：审计语义域（approval.* / fabric.*）+ 前缀过滤 + run/agent 过滤
- tc_g3 fabric_submit 闭环：submit → Router → Adapter → result → 事件落库 →
  再 world_query 可见（领域规则投影）——「提交即世界」
- tc_g4 单一语义源：三个 Transport 形态（trait 直接调用）得到相同语义
  （编译级 + 行为级：Facade 是唯一入口，内部实现不可达）

## 修订日志

| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-16 | 契约定稿：双向模型 + 三语义域 + 最小 Rust API + 内部不暴露原则 + 未来扩展清单 |
| V1.1 | lushaohui | 2026-08-16 | 实现落地：src/facade.rs（三 Service + Facade 聚合）；契约测试 tc_g1–g4 红→绿；CLI Transport（`oaip facade world query` / `audit query` / `fabric submit`，block_in_place 约定）；投影扩展：fabric.* 通信事件（Cognitive）经领域规则可投影进 World State；Bridge.submit 对象引用以请求 payload.object 优先（世界语义由请求方持有） |
| V1.2 | lushaohui | 2026-08-16 | Server Transport + Web UI 落地：`/api/v1/facade/world/query` / `audit/query` / `fabric/submit`（内部 block_in_place 调 Facade；AppState 装配按 runtime flavor 分支——multi_thread 全装配、current_thread 测试环境空 Facade）；/audit 旧端点补 fabric 域；UI 三件套：WorldPanel 投影世界（world.query 关系表 + object/predicate 过滤）、FabricPanel（Agent 拓扑 + 通信事件流 + submit 表单）、时间线 fabric 事件文案（tc_sf1–sf4 红→绿） |
| V1.3 | lushaohui | 2026-08-17 | Phase 8 Project Scope：world/audit/fabric 语义域全部支持 project_id（项目范围世界）；新增 project.create/list 语义域（wb_project 注册表 + ontology 定义）；世界按项目过滤投影（事件 → 项目 Kernel Context，项目隔离）；CLI `oaip facade project seed` 落地 OpenAIP-OS 工程本体（模块/文件/测试 → 项目世界，tc_p1–p3 红→绿） |
| V1.4 | lushaohui | 2026-08-18 | 扩展方法落地（fix/survivor-gaps）：`fabric.cancel`/`fabric.stream`/`capability.discover`/`session.get` 实现为 Facade API（`FabricService` + `Facade` 固有方法）；§4「未来扩展」拆分——四项已落地，world.observe/subscribe 与 audit.trace 仍为未来扩展 |
