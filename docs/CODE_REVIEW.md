# Code Review — openaip-os v0.6.1

> 审查日期：2026-08-14
> 范围：`src/` 15 个 Rust 文件（planner 67KB / storage 57KB / server 46KB 三大热点）+ 前端 `ui/src`（7 个 ts/tsx）。
> 方法：4 路并行 agent 深读 + 逐行核验全部 HIGH 项；`cargo check` 通过、无编译错误。
> 结论：**可运行的扎实骨架，但存在 4 个 HIGH 级一致性/崩溃风险 + 若干安全与规模隐患。**

---

## ✅ 亮点（保留现状）

- 认证 fail-closed：`enforce_auth=true` 时缺令牌一律 401；写操作 RBAC 校验到位。
- SQL 全部参数化，**无注入点**；turso 迁移已完成（无 rusqlite 残留）。
- 前端 token 存 sessionStorage（非 localStorage）；REST 统一 `authFetch` 加 Bearer；渲染全走 JSX+`<pre>`，**无 XSS 面**。
- `agent.rs` 子进程超时正确 `kill+wait` 回收；`thiserror` 错误链一致。

---

## 🔴 HIGH — 数据不一致 / 崩溃（优先修）

| # | 位置 | 问题 | 修复方向 |
|---|------|------|---------|
| H1 | `planner.rs:1192` | **无 Saga 补偿**：`apply_steps` 在首个 `PolicyDenied`/`execute_step` 失败时 `return Err`，已执行的 create/update/link 永不回滚，世界模型停半改态。 | 维护逆操作栈，失败时按序补偿；或文档声明非原子、由调用方负责。 |
| H2 | `planner.rs:271` | `prompt()` 在 async `ConsoleApprover` 内 `block_in_place`+同步 stdin；**current_thread runtime 下 panic**，多线程下掏空 worker。 | 改 `spawn_blocking` 或把审批 IO 提到独立线程；CLI 强制 `multi_thread`。 |
| H3 | `storage.rs:1226-1266` | `link()` **TOCTOU**：先 SELECT 检测重复边再 INSERT，无事务、无 UNIQUE 约束；并发同三元组都过检测 → 重复边（违 WB2007）。 | 加 `(source,relation,target,is_delete=0)` UNIQUE；改 `INSERT ... ON CONFLICT DO NOTHING` + 冲突判定。 |
| H4 | `storage.rs:922-949` | `update()` 乐观锁形同虚设：读 `version` 内存 +1 后 `UPDATE` 不带 `WHERE version=?`；并发 **lost update**。 | `UPDATE ... SET data=?,version=version+1 WHERE id=? AND version=?`，校验影响行数。 |

---

## 🟠 MEDIUM

### 后端（安全 / 规模 / 正确性）
| # | 位置 | 问题 | 修复方向 |
|---|------|------|---------|
| M1 | `server.rs:413-463` | WS 用 `?token=` 查参暴露令牌 → 进日志/历史/Referer。 | 一次性短时效 ticket 或首帧带令牌。 |
| M2 | `server.rs:574/601/649/688/717` | 读接口 `list_trace/get_trace/list_memory/list_tasks/list_evolution` **未 `clamp_view`**，Executive 拿全量 payload（违三层视图设计）。 | 统一套 `clamp_view`/角色门禁。 |
| M3 | `server.rs:487` | 登录**无限流/锁账户**，admin 单账号可爆破（代码自承缺口）。 | 按 IP/用户名计数，失败 N 次锁 M 分钟。 |
| M4 | `storage.rs:1503` | `save_session` **明文 token 存库**，DB 泄漏即所有活动会话可用。 | 存 `SHA-256(token)`，校验时同哈希比对。 |
| M5 | `storage.rs:951/409/425/496/1438/582/693/640` | 列表/快照**全量无 LIMIT/分页**（驱动 GraphRAG `get_object_graph`），增长必 OOM。 | 加 `LIMIT/OFFSET` 或游标分页；快照改流式分批。 |
| M6 | `storage.rs:339/808` | `object_refs LIKE ?` / `count_action_failed` 对 JSON 字段 LIKE：值中 `%`/`_` 被当通配符、`"` 破坏匹配；前导通配符**无法走索引**全表扫描。 | 关系表单独存 object_refs 或 `json_each`；输入预转义。 |
| M7 | `storage.rs:254-272` | 迁移脆弱：`SCHEMA_VERSION="1"` 单常量 + `ALTER ADD COLUMN` **吞错**；列变更只能 `db reset`（**数据全丢**）。 | 版本化迁移脚本（per-version up/down + 回填），错误不静默。 |
| M8 | `trace.rs:209` | `build_timeline` **不按 timestamp 排序**，依赖 DB 返回顺序。 | 排序后再裁剪。 |
| M9 | `trace.rs:169` | `derive_run_state` 仅 Completed/Running，无 Failed/Interrupted；`risk = max(confidence)` 语义反（低置信=高风险）。 | 修状态机 + 风险取低置信。 |
| M10 | `planner.rs:374/385` | `ApprovalStore` 用 `.lock().unwrap()`（poison 即 panic），同文件已有 `lock_mutex` 却没统一用。 | 改用 `lock_mutex`。 |
| M11 | `planner.rs:556-567` | resolve 与 timeout **竞态**；`resolve` 只改内存不落库 → 重启残留 Pending 幽灵审批。 | 状态变更落库；原子 CAS 防竞态。 |

### 前端（ui/src）
| # | 位置 | 问题 | 修复方向 |
|---|------|------|---------|
| U1 | `useTraceStream.ts:34-37,56` | 令牌经 `?token=` URL（呼应 M1）；断线重连**未处理令牌失效**，无效令牌无限重连。 | 检测 auth 失败关闭帧即停重连并回登录。 |
| U2 | `api.ts:38` | `fetch` **无超时/AbortController**，长任务 hang → UI 永久 pending。 | 所有 `authFetch` 加 `AbortSignal`+超时。 |
| U3 | `api.ts:26-31` | `unwrap` 直接 `res.json()`：非 JSON 错误响应抛 `SyntaxError`，信息不可读；403/5xx 未识别。 | 先判 `res.ok`，解析失败兜底为 `Error('服务异常')`。 |
| U4 | `App.tsx:185-190` | `onEvent` 每事件 `void reloadMemory()/reloadTasks()`，高频事件 → 并发大量 REST refetch 互相覆盖 + 打后端。 | 事件侧节流/debounce。 |
| U5 | `view.ts:48` | `status` 与 M9 同源：仅"有 MemoryUpdate 即 Completed"。 | 透传后端 run 状态。 |
| U6 | `view.ts:53-56` | `humanApproval` 只判 `Deny`，漏判 `ApprovalRequired`/`Pending`。 | 同时匹配 Pending/ApprovalRequired。 |
| U7 | `view.ts:75-78` | `risk = max(confidence)` 语义反（同 M9）。 | 风险取 `1 - max(confidence)`。 |
| U8 | `view.ts`/`App.tsx` | 事件未排序即渲染（同 M8）。 | 按 `timestamp` 排序。 |
| U9 | `App.tsx` | 无 Error Boundary，渲染抛错即整页白屏。 | 顶层加 `ErrorBoundary`。 |
| U10 | `api.ts:81` | `getTimeline` 已定义但 App 未使用（走 WS+buildTimeline），疑似死代码。 | 删除或确认用途。 |

---

## 🟡 LOW
- `odl.rs:373` `parse_value_expr` 字符串不校验闭合/转义（`"a\"b"` 变 `a\"b`）。
- `codegen.rs:75` 裸 JSON 未转义 + 未检测**重复 object/property** → 生成同名 struct 编译失败。
- `action.rs:169-171` 边已存在时 `effect.linked` 仍 push（重复上报）。
- `planner.rs:816` `extract_named_param` 只取首词，`name=Alice Bob` 丢"Bob"。
- `planner.rs:1406-1416` `MemoryUpdate` 硬编码"挽留"语义，污染非 retention 目标审计。
- `planner.rs:1278` `_ = intent_dec` 计算后丢弃，每轮多一次无意义 LLM 调用。
- `evolution.rs:212` `parse_run_ids` `unwrap_or_default` 静默吞错，破坏可追溯。
- `auth.rs:150/176` `Argon2::default()` 参数未显式（建议内存≥64MiB, t=3, p=4）。
- `server.rs` 无 CORS/安全头；`is_public` 精确匹配（fail-closed 故安全，仅健壮性）。
- `storage.rs:195` `is_no_such_table` 靠英文串匹配，跨版本不稳。

## 🏗 架构 / 模块化
- `odl.rs` `parse_odl` ~279 行 + 4 个近同 `flush_*` + 三处重复 `- field/type` 解析 → 违反 AGENTS.md "400 行模块化"原则。建议拆为按 Section 子解析器 + 参数化冲刷。
- `planner.rs::run_goal` ~218 行单函数；`planner.rs` 整体 1700 行，建议按阶段（parse→plan→policy→execute）拆分。
- `tool_backend.rs` 仅 `NullBackend`，trait 未强制超时/响应校验，未来 HTTP 后端须补 `timeout` + 状态码校验。

---

## 🔗 前后端同源"成对 bug"（都该修）
- 状态机缺 Failed/Interrupted：`trace.rs:169` + `view.ts:48`
- risk 语义反：`trace.rs:232` + `view.ts:75`
- 事件不排序：`trace.rs:209` + 前端渲染
- WS 令牌入 URL：`server.rs:413` + `useTraceStream.ts:35`

---

## 🎯 建议修复顺序
1. **H3 / H4**（storage 事务 + UNIQUE）——改动集中、收益最大、防并发数据损坏。
2. **H2**（`spawn_blocking`）——防单线程 runtime panic。
3. **H1**（Saga/补偿或明确文档）——一致性根本问题。
4. **M1 / U1 / M3**（WS 令牌、登录限流）——安全面。
5. **M4**（会话哈希）——DB 泄漏兜底。
6. **M5 / M6 / M7**（分页、LIKE-JSON、迁移脚本）——规模与可运维性。
7. **U2 / U3 / U4**（fetch 超时、错误解析、事件节流）——前端健壮性。
