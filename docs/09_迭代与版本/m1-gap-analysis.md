# M1a 差集分析 — V1 planner 退役功能对齐清单

| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-09-005 |
| 所属模块 | OpenAIP-OS-TechDebt |
| 版本 | V1.0 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-16 |
| 前置 | tech-debt-reduction-plan.md（M1 里程碑） |
| 状态 | 分析完成，待 M1b 契约测试 |

## 1. 目的

逐维度对比 V1 `planner::run_goal` 与 V2 cognitive loop（`run_until_done`）的能力覆盖，
产出 M1b 契约测试清单与 M1d 删除范围清单。所有结论基于 2026-08-16 代码现场核查。

## 2. 已覆盖（V2 已具备等价能力，零补齐工作）

| # | 维度 | V1 实现 | V2 等价实现 | 结论 |
|---|------|---------|-------------|------|
| C1 | Policy 前置条件 | `Policy::check_action` precondition（AND 字段比较） | `compose::Policy::check`（preconditions AND） | ✓ 等价 |
| C2 | require_approval 挂起/恢复 | `PolicyApprover` + `ApprovalStore` + 超时回收 | `step_with_gate` → `Waiting` + `relate(Approval)` 恢复 | ✓ 机制等价 |
| C3 | Evolution AutoApprove 5 闸门 | `may_auto_approve`（REQ-018） | `EvolutionGate::gate()` → `step_with_gate(gate_pass)` | ✓ 语义等价 |
| C4 | Memory 沉淀/召回 | `wb_memory` 表 + hits 累加 | `agent.memory`（record/recall）+ `CognitiveStore` 持久化（Phase 10） | ✓ 等价 |
| C5 | 多波重规划 | wave 循环（MAX=8，世界变化后重规划） | 每轮 `run_once` Observe→Decide（每步验证世界） | ✓ V2 更强 |
| C6 | 动作幂等 | upsert / 边去重 | 由 capability 实现保证（compose 层） | ✓ 等价 |

## 3. 差集（切换入口前必须补齐，M1b 契约测试对象）

### G1 认知 Trace 事件流（最大缺口）✅ 已落地（commit cecc44f）

- V1：run_goal 全链路 emit 12 种认知事件（Goal / Intent / OntologyQuery / MemoryRecall /
  Reasoning / Planning / PolicyCheck / ToolBackend / ActionFailed / Action / MemoryUpdate /
  Reflection）→ `wb_trace_event(kind=cognitive)` → UI 时间线 + WS 流 + evaluation 桥接。
- V2 原状：`run_once` 零事件。切换后 UI 时间线空白、`v0_9_ws_clamp` 失效。
- 已落地：`AgentLoop::with_trace(run_id, sink)`——调用方持有 sink，事件与 V1 同源
  （Goal/OntologyQuery/Reasoning/Intent/Planning/PolicyCheck/Action/PendingApproval/
  ActionFailed/MemoryUpdate）；resume 续跑扫描去重（Goal 不重复）；空世界 object_refs
  兜底 "world"；`runtime::new_trace_id/now_ts` 转 pub。sink → `wb_trace_event` 落库在
  M1c 入口接线时做。
- 契约测试：`tests/v2_parity/trace_events.rs` 4 例全绿（闭环全流水线 / 审批挂起恢复 /
  拒绝零记忆 / 默认关闭）。基线 356 → 360。

### G2 审批适配器（CLI + HTTP 两条腿）✅ 原语已落地（commit eed2f43）

- V1：`PolicyApprover` 三实现——`ConsoleApprover`（stdin y/N + 决策缓存）、`AutoApprover`、
  `ServerApprover`（挂起 + `GET /run/:id/approvals` + `POST /run/:id/approval/:req_id` + 超时 Expired）。
- V2 原状：只有 `Waiting` + `relate(Approval)` 机制，无外部适配器。
- 已落地（库级原语，适配器薄壳在 M1c 接线）：
  - `cognitive/approval.rs`：`ApprovalRequest`（类型化挂起请求，AgentLoop Waiting 时经
    `with_approvals(run_id, sink)` 注册）/ `resolve`（以 Fiber 状态为真相源，非 WAITING →
    AlreadyResolved 拒绝二次裁决）/ `expire`（`approval.expired` 与 Rejected 区分）/
    `DecisionCache`（`action@target` 只询问一次，V1 ConsoleApprover 语义）。
  - `kernel::expire_approval`：Waiting → Cancelled + `approval.expired` 审计（tc023d 语义）。
  - 待 M1c：CLI stdin 交互壳（用 DecisionCache + resolve）、HTTP 端点壳（请求 sink → 现有端点协议）。
- 契约测试：`tests/v2_parity/approval.rs` 4 例全绿（注册+批准恢复 / 拒绝防重 / 超时区分 /
  缓存单次询问）。基线 360 → 364。

### G3 Pi 真实引擎迁移

- V1：`AgentEngine` trait + `PiAgentEngine`（agent.rs 子进程管理 + 超时 kill+wait）。
- V2：`CognitiveEngine` trait 只有 `FakeEngine` / `NoOpEngine`，Pi 适配口未实现。
- 补齐：`PiAgentEngine` 子进程封装迁移为 `cognitive/pi.rs::PiEngine: CognitiveEngine`
  （decide = 复用 V1 intent 分类 + 能力选择 + 决策生成的 prompt 编排）；
  `AgentEngine` trait 与 `WorldSnapshot` 随 planner 删除。
- 测试：`tests/v2_parity/pi_engine.rs`——进程健康检查、超时回收、decide 形状（复用 v0_3 的 Fake 断言模式，真实 Pi 用例保持 ignore）。

### G4 审批审计落库（REQ-018 可审计承诺，已部分覆盖）

- V1：`wb_approval_audit` 表 + `GET /evolution/audit` + AutoApprove 可撤销 API。
- V2 已覆盖（经 tc_e6/tc_e7 验证）：gate 放行 emit `evolution.gate_approved` 审计事件
  （进 Kernel 事实轨）；撤销 = `EvolutionPermissionState::Revoked` 吸收态（revoke 后不复活）。
- 剩余缺口：① 人工审批路径（relate Approval → 放行）的审计事件种类确认/补齐；
  ② 审计查询入口（现有 `GET /evolution/audit` 读 V1 表，需改读事实轨）。
- 测试：`tests/v2_parity/approval_audit.rs`——gate 放行与人工审批均有可回放审计事实；
  撤销吸收态在审计里可查。

### G5 「无失败」数据源切换 ✅ 已确认（characterization，commit d1e86b0）

- V1：`count_action_failed` 从 trace 表统计 `ActionFailed` 事件喂给 AutoApprove 闸门。
- V2 现状核实：`EvolutionGate::check` 的 `failed == 0` 条件读 evolution2 分账
  （Success/Failed/Denied/Compensated 账本），非 trace 计数；分账经
  `snapshot()/restore()` + CognitiveStore 持久化（tc_cp2 已覆盖往返）。
- 结论：数据源切换在 V2 已天然完成，无需改产品代码；新增 3 例契约测试钉住跨快照语义
  （失败阻断跨重启 / 纯成功放行 / 恢复后失败即收回）——`tests/v2_parity/gate_source.rs`。
  删除 V1 后 `count_action_failed` + `ActionFailed` 事件计数随 planner 一并移除。

### G6 run 状态映射与持久化 ✅ 语义搬迁完成（commit d1e86b0）

- V1：`RunRecord{status, human_approval}` + `wb_run` 表 + 重启恢复；UI `deriveState` 依赖。
- 已落地：`RunStatus` 与 `generate_run_id` 迁至 `session/run.rs`（planner 退役后存活，
  planner 暂时 re-export，M1d 收口消费方 import）；
  `fiber_to_run_status` 全映射 13 变体（Waiting/Paused=Running、补偿三态+Cancelled=Failed、
  Interrupted 独立）；`LoopOutcome::to_run_status` → (status, human_approval)。
- 持久化：`storage/runs.rs`（wb_run 表 + rebuild/Interrupted 恢复）已是 String 状态、
  与 planner 解耦——M1c 入口接线直接用；v0_8 重启恢复测试继续有效。
- 契约测试：`tests/v2_parity/run_status.rs` 3 例全绿（映射表 / outcome 映射 / id+字符串往返）。

### G7 health 数据源

- 现状：`LoopHealth` 的 `total_runs` 来自 `wb_run` 表（runs.len()）。
- 影响：M1d 删 `wb_run` 后 closure_rate 分母缺失。
- 补齐：total_runs 改从 Session Log / Evaluation 表派生（I1 State=Reduce(EventLog) 原则）。
- 测试：`tests/v2_parity/health_source.rs`——删表后 LoopHealth 六指标仍可计算且与基线一致。

### G8 `--via-workers` 桥的 Thinker

- 现状：`cmd_agent_run(via_workers=true)` 走 `Planner::plan()` 产出 PlanStep → worker 任务。
- 影响：删除 planner 后此桥断。
- 补齐：Thinker 改为 V2 路径——`CognitiveEngine::decide`（或 skill-based planner）
  产出 Decision → worker capability 映射；`PlanStep::worker_capability` 映射逻辑迁至
  worker 侧或 compose 层。
- 测试：`tests/v2_parity/via_workers.rs`——goal → Thinker 决策 → worker 执行 → 结果，
  与现 bridge_v2 / worker_v2 测试不重复。

### G9 测试迁移（v0_3_* → v2_parity）

- `tests/v0_3_agent.rs`（12+ 用例：闭环/审批/记忆/进化/失败）、`v0_3_policy.rs`、
  `v0_3_evolution.rs`、`v0_3_approval.rs`（HTTP 审批）、`v0_3_server.rs`、
  `v0_9_ws_clamp.rs`（WS 视图钳制）、`tests/common/mod.rs`（run_goal 辅助）。
- 动作：行为逐条改写为 V2 契约测试；v0_7_auth / v0_8_persistence 保留
  （认证与持久化能力存活，只改其对 run 的依赖面）。
- 出口：v2_parity 全绿后，v0_3_* 与 common 的 run_goal 依赖整体删除。

### G10 UI 面板数据源

- `MemoryPanel`（GET /memory）、`TaskPanel`（GET /tasks）、`EvolutionPanel`（GET /evolution）
  读 V1 表；`V2Panel` 已是 V2。
- 动作：三面板改接 V2 数据源（Memory/CognitiveStore、Task/Fiber、Evolution/evolution2），
  或删除并并入 V2Panel——按「最少面板数」原则在 M1c 定案。
- 测试：UI 侧 vitest 契约（数据形状）更新。

## 4. M1d 删除范围清单（依据调用面核查）

### 模块

| 模块 | 消费面（已核查） | 处置 |
|------|------------------|------|
| `src/planner/`（mod/run/approval/intent/policy） | CLI agent run、server/run.rs、via_workers 桥、v0_3 测试 | 删除；`generate_run_id` 迁 `src/session/` |
| `src/action.rs` | 仅 planner + server 门面 | 删除 |
| `src/agent.rs`（AgentEngine/PiAgentEngine） | planner + CLI | Pi 子进程封装迁 `cognitive/pi.rs`（G3），trait 删除 |
| `src/runtime.rs`（RuntimeKernel 门面） | server/mod.rs + CLI | 删除；server 直连 V2 Kernel + LibSQLStorage |
| `src/tool_backend.rs`（NullBackend） | 仅 planner | 删除 |

### 存储与表（codegen schema 同步）

| 表 | 用途 | 处置 |
|----|------|------|
| `wb_run` | run 状态（G6 换新落库） | 删除，G6 新表替代 |
| `wb_memory` | V1 记忆（C4 → CognitiveStore） | 删除 |
| `wb_approval` | V1 审批挂起（G2 → Waiting 注册表） | 删除 |
| `wb_pattern` / `wb_proposal` / `wb_learned` | V1 Evolution（M2 归一后删） | **M2 处理，M1 不动** |
| `wb_approval_audit` | REQ-018 审计（G4 → Session Log） | **M2 处理** |

### 端点与 UI

| 端点/面板 | 处置 |
|-----------|------|
| `GET /memory` / `GET /tasks` / `GET /evolution`（views.rs） | 改 V2 数据源或删除（G10） |
| `GET /run/:id/approvals` + `POST /run/:id/approval/:req_id`（approvals.rs） | 协议保留，存储换 Waiting 注册表（G2） |
| `GET /evolution/audit` + AutoApprove 撤销 API | M2 处理 |
| MemoryPanel / TaskPanel / EvolutionPanel | G10 定案 |

## 5. 待确认（M1c 设计时定案）

1. G4 审计落库位置：Session Log 事实轨 vs 独立表——倾向 Session Log（I1 原则），M2 定。
2. G10 面板合并：三面板并入 V2Panel vs 独立保留——倾向并入，减少 UI 面。
3. server 门面形态：`AppState` 从 RuntimeKernel 切换为（V2 Kernel + LibSQLStorage + CognitiveStore）三件套。

## 修订日志

| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-16 | 初稿：C1-C6 覆盖确认 + G1-G10 差集 + M1d 删除清单 |
