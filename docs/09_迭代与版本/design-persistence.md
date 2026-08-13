# 技术设计：运行态持久化与重启恢复（V0.7 · 对应 P0 风险「运行态全在内存」）

| 版本 | V0.1（设计稿，待评审） |
| --- | --- |
| 关联风险 | P0：`RunRecord`(server.rs:39) / `ApprovalStore`(planner.rs:311) / EventBus 全进程内，重启丢失 |
| 目标版本 | V0.7 |
| 状态 | 已落地 · V0.7（提交 ec...persist） |
| 作者 | lushaohui |

---

## 1. 背景与问题

- `RunRecord` 存于 `AppState.runs: Mutex<HashMap<String, RunRecord>>`（server.rs:59）——运行中任务状态重启即失。
- `ApprovalStore.requests: Mutex<HashMap<..., ApprovalEntry>>`（`tx: Option<oneshot::Sender<bool>>`）——待审批请求重启即失，且 `oneshot` 唤醒信道**天生不可跨进程持久化**。
- EventBus（server.rs:687）仅进程内实时推送；历史已在 `wb_trace_event` 落库，重连可回放。

后果：服务重启后，进行中的 run 状态、待审批请求、实时订阅全部丢失，只能靠 `wb_trace_event` 事后回放。

## 2. 目标 / 非目标

**目标**

- run 生命周期持久化（`wb_run`）：`run_start` → 完成/失败，状态不丢。
- 审批请求持久化（`wb_approval`）：列表/裁决 API 跨重启可用，并保留审计。
- 重启恢复：重建 `runs` 内存映射；对崩溃残留做一致处理。

**非目标**

- **不持久化 WS 订阅 offset**：EventBus 仍内存；客户端断线重连从 `wb_trace_event` 回放（`?after=` 已支持）。
- 不做 run 执行的断点续跑（Planner 不支持，且风险高）；恢复只重建「状态视图」，不恢复「执行现场」。

## 3. 设计

### 3.1 数据表（追加到 `canonical_schema()`）

```sql
CREATE TABLE IF NOT EXISTS wb_run (
  id            TEXT PRIMARY KEY,        -- run_id
  agent_id      TEXT NOT NULL,
  goal          TEXT NOT NULL,
  status        TEXT NOT NULL,           -- Running | Completed | Failed | Interrupted
  human_approval INTEGER NOT NULL DEFAULT 0,
  created_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  finished_at   TEXT
);
CREATE INDEX IF NOT EXISTS idx_run_agent ON wb_run(agent_id);
CREATE INDEX IF NOT EXISTS idx_run_status ON wb_run(status);

CREATE TABLE IF NOT EXISTS wb_approval (
  id          TEXT PRIMARY KEY,          -- req_id
  run_id      TEXT NOT NULL,
  agent_id    TEXT NOT NULL,
  step_id     TEXT NOT NULL,            -- action@target_id
  description TEXT,
  state       TEXT NOT NULL,            -- Pending | Approved | Rejected | Expired
  created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  resolved_by TEXT,                     -- 裁决人（AuthUser.id，预留；V0.7 前可为空）
  resolved_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_appr_run ON wb_approval(run_id);
CREATE INDEX IF NOT EXISTS idx_appr_state ON wb_approval(state);
```

### 3.2 RunRecord → wb_run

- `run_start`：`runs` 内存插入 `Running` **同时** `INSERT wb_run(status='Running')`。
- 完成/失败：更新 `runs` 内存 **同时** `UPDATE wb_run SET status=?, finished_at=?`。
- `runs` 内存映射继续服务热路径（WB2004/2005 快速反馈）；`wb_run` 为持久真相源。
- 查询 run 状态优先级：内存 `runs` 优先，缺失时回退 `wb_run`（覆盖重启后内存为空的情况）。

### 3.3 ApprovalStore → wb_approval

- `register()`：`INSERT wb_approval(state='Pending')` **并** 保留 `oneshot` `tx` 在内存 `HashMap`（按 `req_id`）。`tx` 不可持久化，这是预期——持久化目的是**审计 + HTTP 列表/裁决跨重启可用**。
- `list_for_run` / `resolve_approval`：读写改走 `wb_approval`（同现有 `ApprovalRequest` 字段映射）。
- 裁决：`UPDATE wb_approval SET state=?, resolved_by=?, resolved_at=?` 后，唤醒内存 `tx`（若存在）。重启后内存 `tx` 为空 → 唤醒无操作，但 DB 状态已持久，UI 列表正确。

### 3.4 重启恢复（`serve` 启动时执行）

```
fn recover(state: &AppState) {
    // 1) 重建 runs 映射
    for r in storage.list_runs() {
        state.runs.lock().insert(r.id, RunRecord{ status: r.status, human_approval: r.human_approval });
        // 2) 崩溃时仍 Running 的 run → 无活执行器 → 标 Interrupted
        if r.status == Running {
            storage.update_run(r.id, Interrupted);
            state.runs 同步更新;
        }
    }
    // 3) 待审批 Pending：run 当前在执行（内存命中）→ 保留 Pending；orphan → Expired
    for a in storage.list_approvals_by_state(Pending) {
        if state.runs 中存在 a.run_id 且 status==Running {
            // 保留 Pending（极少：刚好启动时有活 run）
        } else {
            storage.update_approval(a.id, Expired);   // 与 ServerApprover 超时语义一致
        }
    }
}
```

- `Interrupted` 作为 `RunStatus` 新变体（区别于 `Failed`）：表示「进程崩溃未正常结束」，timeline 明确展示。
- orphan Pending → `Expired` 与现有超时回收语义一致，避免 UI 幽灵待审批（沿用 TC-023c/d 约定）。

### 3.5 事务边界（顺带缓解 P1「对象写+事件写非原子」）

- 新增 `LibSQLStorage::with_tx(|tx| -> Result<(), E>)` 包裹「对象写 + `emit_cognitive` 事件写」为单事务（libsql 支持 `BEGIN/COMMIT`）。
- `emit_cognitive` / ObjectEngine 写路径改走 `with_tx`：任一步失败整体回滚，杜绝「对象落库成功、事件丢失」。
- 影响面：`runtime::emit_cognitive`、ObjectEngine 实现；需确认 libsql 当前版本事务 API（turso 已支持）。

### 3.6 API 变更

- 外部 API **不变**；`list_approvals` / `resolve_approval` / `timeline` 透明切到 DB 后端。
- `serve` / `serve_with_timeout` 启动处插入 `recover(state)`（测试注入 `approval_timeout` 不影响恢复路径）。

### 3.7 测试矩阵（建议 TC-persist-a~e）

| 用例 | 预期 |
| --- | --- |
| a 触发 run | `wb_run` 先 `Running`，完成后 `status=Completed` |
| b 重启后查 run | `runs` 内存重建，状态与 DB 一致 |
| c 崩溃时 Running 的 run | 重启标记为 `Interrupted` |
| d 待审批 Pending 跨重启 | run 存活→仍 Pending；orphan→`Expired` |
| e 对象+事件同事务 | 注入写事件失败 → 对象与事件均回滚（无半落） |

## 4. 设计分叉（已定稿 · 2026-08-13 评审结论）

| # | 分叉 | 结论 | 理由 |
| --- | --- | --- | --- |
| P1 | EventBus 持久化 | 否 | `wb_trace_event` 已落库，WS `?after=` 回放够用；订阅 offset 属未来多节点问题 |
| P2 | 崩溃 Running → `Interrupted` | 是（新状态） | 语义准确，与 `Failed` 区分；timeline 同步加该状态展示 |
| P3 | orphan Pending → `Expired` | 是 | 与 TC-023c/d 超时语义一致，避免 UI 幽灵待审批 |
| P4 | 对象+事件同事务 | 本迭代一并做，但**独立小 PR** | 数据一致性隐患，拖久更难改；不混进 run/approval 持久化 |

**评审补充建议（已吸收进设计）：**

- `with_tx` 实现前先验证 libsql 0.9 在同一 `Connection` 上并发 async 调用的事务行为；连接共享时可能需独立连接或串行化事务。
- 重启后无"活 run"可恢复 → 简化恢复分支：**所有 Running → `Interrupted`，所有 Pending → `Expired`**（去掉"run 存活→Pending 保留"死代码分支，少一条分支少一个测试）。

## 5. 修订日志

| 版本 | 作者 | 日期 | 说明 |
| --- | --- | --- | --- |
| V0.3 | lushaohui | 2026-08-13 | 实现落地：canonical_schema 加 wb_run/wb_approval；storage 加 RunRecord/ApprovalRecord + insert/update/list/get + recover_persisted_state（Running→Interrupted、Pending→Expired 简化分支）；server 在 run_start 持久化 run、ServerApprover 持久化 approval、list_approvals 合并内存+DB、resolve_approval 持久化裁决、timeline DB 回退、启动时 recover() 重建 runs 映射；RunStatus::Interrupted + ApprovalState 持久互转。TC-persist-a~d 全绿（48 既有 + 7 auth 保持绿）。P4 with_tx 按评审结论**独立 PR**，本增量不做（见 §3.5 / TC-e） |
| V0.2 | lushaohui | 2026-08-13 | 评审定稿：P1–P4 结论落表；补充建议（with_tx 前验证 libsql 事务行为、恢复分支简化）吸收；排期 Auth 之后 |
| V0.1 | lushaohui | 2026-08-13 | 初稿：wb_run/wb_approval DDL + RunRecord/ApprovalStore 持久化映射 + 重启恢复（Interrupted/Expired 语义）+ 事务边界缓解 P1 + TC 矩阵；列 4 项分叉 |
