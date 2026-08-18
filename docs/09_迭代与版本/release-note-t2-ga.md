# OpenAIP-OS T2 发布说明：Agent Fabric「异步 + 实时」正式交付（GA）

| 项 | 内容 |
|----|------|
| 里程碑 | T2（Agent Fabric 异步契约 + 实时可观察） |
| 版本范围 | V0.11.0 → V0.14.0 |
| 发布日期 | 2026-08-18 |
| 发布状态 | T2 核心（异步 + 实时）已合 master；shell-exec 提供方（B1，commit 20d947a）与加固增量（S1/S2/S4，含 shutdown 进程组回收修复）均已合 master 且 `make check` 全绿——可对外正式宣布 |

## 一句话结论
Agent 任务的**提交不再阻塞执行**、任务的**全生命周期事件实时推送 WS**（治理面板免轮询）、**取消在任意窗口生效**、**错误不再静默吞掉**；并且 agent 终于能**真正执行 shell 命令**（此前是能力市场假广告导致的死穴）。

## 这次交付了什么

### 1. 异步契约（提交即返回，执行后台跑）
- `Bridge::submit` 立即返回 `running` + `correlation_id`，实际执行经 `spawn_blocking` 后台运行（fire-and-forget）。CLI / HTTP 调用方不再被 agent 执行时长阻塞。
- 配套 `FabricService` 持 tokio Handle 驱动后台任务，会话状态机不退化、不死锁。

### 2. 实时可观察（fabric 事件推 WS，治理面板免轮询）
- `StorageWorldSink` 落库后旁路扇出到 `event_tx`（broadcast channel）；`Facade` 自建频道、`AppState` 复用同一频道，WS `/stream` 实时收到 `fabric.*` 事件。
- WS 过滤层放行「双 None + `fabric.` 前缀」的世界级事实——修复了广播已接上、WS 却收不到的断链（E2E 实测订阅者曾收到 0 条）。
- 断线重连走 `after_id` 游标从存储（权威）补齐，重连不丢数据、最终一致。

### 3. 取消全窗口覆盖
- late-cancel（adapter `killed` mutex，spawn 后判）+ early-cancel（`cancel_requested` 标记集，spawn 前判）互补，覆盖 submit 返 running 与子进程进 running map 之间的竞态窗口——点了取消就一定生效。

### 4. 错误可见
- 后台执行 `stream` 失败时先发 `fabric.task_failed`（带错误文本）+ 会话终态，再返 Err；不再被 `run_in_background` 的 `let _ = e` 静默吞掉导致 correlation 永久悬挂、CLI 误导「超时未终局」。

### 5. shell-exec 提供方（B1 · 根治执行链死穴）
- `run_shell` → `shell.execute` 现路由到专用 `/bin/sh` 提供方（复用已验证的进程适配器通道）。
- 从 pi / dsh / deepseek 三个 harness 的 manifest **撤下 `shell.execute` 假广告**（它们不真提供 shell），路由确定性随之提升（消除同优先级 HashMap 插入序竞态）。
- `run_shell` 仍 `mark_high_risk`：人工审批闸门 + 审计 + 可取消兜底不变。

### 6. 加固增量（S1 / S2 / S4）
- **S1** CLI 轮询超时可配：`oaip facade fabric submit --timeout <SECS>`（默认 30，`0` = 提交后立即返回不等待）。
- **S2** 优雅关闭回收孤儿 job：服务收到 SIGINT / SIGTERM 时 drain 所有运行中的后台子进程，对整个进程组发 SIGKILL（含 `sh -c` 派生的孙子进程）——避免孙子进程继承 stdout 管道导致 EOF 不来、stream 永久阻塞，也杜绝 shutdown 遗留孤儿后台进程（与 `cancel` 同机制）。
- **S4** broadcast lag 语义声明：实时 best-effort（有界缓冲）+ 存储权威补齐，明确「瞬时实时缺口 ≠ 数据丢失」。

## 诚实的工程叙事（为什么现在才宣布）
T2 的代码其实更早就在 master 上，但**早先的「完整交付」宣称不成立**——独立深度 review 发现 2 Critical + 2 High 接线 / 语义缺口：

- C1 广播层接上，但 WS 过滤层把 fabric 事件全丢弃；
- C2 审计查询固定 `ASC LIMIT 500`，终局事件（最新）永远在窗口外，CLI 轮询必漏；
- H1 `run_in_background` 吞错，correlation 永久悬挂；
- H2 early-cancel 竞态，submit 与子进程进 running map 之间 cancel 命中 0。

逐一修复后，又通过**真实端到端验证**揪出第 5 处洞（C1 完整性：WS 订阅者收到 0 条 fabric 事件 —— 广播虽接上，过滤层仍断链）。直到 E2E 实证通过，我们才把「异步 + 实时」的交付承诺坐实。这是一次「不 premature announce」的纪律兑现。

## 发布状态（请发布前核对）
| 项 | 状态 |
|----|------|
| T2 异步 + 实时核心（V0.12.0 + C1 完整性修复 `0f4792e`） | ✅ 已合 master |
| shell-exec 提供方（V0.13.0 / B1，commit `20d947a`） | ✅ 已合 master |
| 加固增量（V0.14.0 / S1 / S2 / S4） | ✅ 已合 master + `make check` 全绿 |

> T2 完整包（核心 + B1 + 加固）均已合 master 且门禁全绿，可对外正式宣布。

## 升级 / 使用提示
- CLI 异步提交（超时可配）：
  ```bash
  oaip facade fabric submit \
    --session <S> --sender <A> --intent run_shell \
    --payload '{"args":["-c","echo hi"]}' --timeout 60
  ```
- 治理面板实时观察：WS `/stream` 现已收到 fabric 全生命周期事件，无需轮询。
- 已知边界：S2 回收用 SIGKILL（与 `cancel` 一致，零新依赖）；如需严格 SIGTERM 优雅退出需引入 `libc` / `nix` 发信号 —— 已列入后续增量，不阻塞本次交付。

## 契约测试钉死（防回归）
- `tc_f11`（H1 失败时发 `fabric.task_failed`）、`tc_f12`（H2 early-cancel 生效）、`list_events_newest_first_*`（C2 倒序取终局）
- `tc_sf5`（C1 完整性：fabric 事件实时达 WS）、`tc_sf6`（B1：run_shell 经 shell-exec 真跑 `/bin/sh` 终态 completed）

---
*关联文档：`docs/09_迭代与版本/version-changelog.md`；原始 review 与 E2E 实证记录见项目工作日志 2026-08-18。*
