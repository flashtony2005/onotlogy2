# M1a 差集分析 — V1 planner 退役功能对齐清单

| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-09-005 |
| 所属模块 | OpenAIP-OS-TechDebt |
| 版本 | V1.0 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-16 |
| 前置 | tech-debt-reduction-plan.md（M1 里程碑） |
| 状态 | M1 全里程碑关闭（M1a 分析 + M1b 契约 + M1c 切换 + M1d 删除均已完成，G1–G10 全部钉绿） |

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

### G3 Pi 真实引擎迁移 ✅ 已落地（commit d56057f）

- V1：`AgentEngine` trait + `PiAgentEngine`（agent.rs 子进程管理 + 超时 kill+wait）。
- 已落地：`cognitive/pi.rs::PiEngine: CognitiveEngine`——`decide` 是同步 trait，
  故用 `std::process` 阻塞调用 + `try_wait` 轮询超时 kill+wait 回收（零 trait 改动，
  server 侧可整体包 spawn_blocking）；capability=null → None（D5 路径）、
  视图外能力 → UnknownCapability（fail-safe）、缺失/崩溃/超时 → Engine（WB2004 同源）。
  `which_pi`/`strip_code_fence` 迁至 pi.rs（V1 agent.rs 复用同一实现，M1d 删 agent.rs）；
  `which_pi` 补存在性检查（PI_BIN 悬空路径不得误报 healthy）。
- 契约测试：`tests/v2_parity/pi_engine.rs` 6 例全绿（结构化决策 / none / 未知能力拒绝 /
  不可用 / 超时杀进程 / health 探测）+ 4 个假 pi fixtures。基线 370 → 379。

### G4 审批审计落库（REQ-018 可审计承诺）✅ 审计事实补齐（commit 1d4526f）

- V1：`wb_approval_audit` 表 + `GET /evolution/audit` + AutoApprove 可撤销 API。
- V2 已覆盖（经 tc_e6/tc_e7 验证）：gate 放行 emit `evolution.gate_approved` 审计事件
  （进 Kernel 事实轨）；撤销 = `EvolutionPermissionState::Revoked` 吸收态（revoke 后不复活）。
- 已补齐：relate(Approval) 恢复路径落 `approval.granted`（by=关系 source）——
  外部事件统一入口与 `k.approve` 同源可回放；全链审计种类（requested/granted/
  rejected/expired/gate_approved）各司其职。
- 剩余（M2 定案）：审计查询入口——现有 `GET /evolution/audit` 读 V1 表，改读事实轨
  （与 wb_approval_audit 表的处置一起在 M2 归一）。
- 契约测试：`tests/v2_parity/approval_audit.rs` 3 例全绿（relate granted / 显式 approve /
  拒绝与超时种类区分）。

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

### G7 health 数据源 ✅ 计划修订并落地（commit d56057f）

- 现状：`LoopHealth` 的 `total_runs` 来自 `wb_run` 表（runs.len()）。
- **计划修订**（基于代码证据的最简方案）：`storage/runs.rs` 已是 String 状态、与 planner
  零耦合——wb_run **不删除**，重定位为 V2 run store；M1c 起由 V2 入口写入
  （insert_run / update_run_status 用 `session::run::RunStatus` 字符串）。
  原「改从 SessionLog/Evaluation 派生」方案废除（增加派生复杂度且 CLI 路径无评估覆盖）。
- 契约测试：`tests/v2_parity/health_source.rs` 2 例全绿——六指标从 V2 写入数据派生
  （total_runs=3、closure=1/3、action_success=1、world_change=1/3）+ 空库零分母保护。

### G8 `--via-workers` 桥的 Thinker ✅ 已落地（commit 1d4526f）

- 现状：`cmd_agent_run(via_workers=true)` 走 `Planner::plan()` 产出 PlanStep → worker 任务。
- 已落地：`worker/thinker.rs::Thinker`——`CognitiveEngine::decide` 循环的纯规划形态
  （不执行、无世界副作用）；防环收敛（None / 重复能力 / 能力数上限）；
  `Decision.capability` 与 Worker 能力市场同名直映（compose 命名已是点分形式），
  `PlanStep::worker_capability` 映射随 planner 一并删除；`to_tasks` 保序。
- M1c 动作：`cmd_agent_run(via_workers=true)` 改走 Thinker + WorkerOrchestrator（不再触 planner）。
- 契约测试：`tests/v2_parity/via_workers.rs` 4 例全绿（全链执行 / 防环收敛 / 空任务 / 保序）。

### G9 测试迁移（v0_3_* → v2_parity）

- `tests/v0_3_agent.rs`（12+ 用例：闭环/审批/记忆/进化/失败）、`v0_3_policy.rs`、
  `v0_3_evolution.rs`、`v0_3_approval.rs`（HTTP 审批）、`v0_3_server.rs`、
  `v0_9_ws_clamp.rs`（WS 视图钳制）、`tests/common/mod.rs`（run_goal 辅助）。
- 动作：行为逐条改写为 V2 契约测试；v0_7_auth / v0_8_persistence 保留
  （认证与持久化能力存活，只改其对 run 的依赖面）。
- 出口：v2_parity 全绿后，v0_3_* 与 common 的 run_goal 依赖整体删除。

### G10 UI 面板数据源 ✅ 定案（M1c/M1d 执行）

- `MemoryPanel`（GET /memory）、`TaskPanel`（GET /tasks）、`EvolutionPanel`（GET /evolution）
  读 V1 表；`V2Panel` 已是 V2。
- **定案（最少面板数）**：三面板 + 三端点 + UI 导航三个视图标签（memory/tasks/evolution）
  在 M1d 一并删除，`V2Panel` 承担 V2 状态展示；记忆如需展示，M1c 在 V2Panel 内接
  CognitiveStore 数据（MemoryItem 与 V1 MemoryRecord 同形：label/payload/hits）。
  Evolution 审计/撤销 API 随 M2（evolution.rs 删除）移除。
- 证据：TaskPanel 期望 V1 域对象 RetentionTask，而 wb_task 现为 TaskDefinition 存储
  （evaluation 用）——域语义已死，面板无数据可展示；MemoryPanel 的数据源 wb_memory
  随 M1d 删除。
- 测试：UI view.ts 的 memory/tasks/evolution 分支随 M1d 删除（view.test.ts 同步）；
  时间线行为已由 v2_parity g9b/g9c 钉住（UI 无需改动即消费 V2 事件）。

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
| `wb_run` | run 状态（**计划修订 G7**：重定位为 V2 run store） | **保留**，M1c 起由 V2 入口写入（storage/runs.rs 已与 planner 解耦） |
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
2. G10 面板合并：三面板并入 V2Panel vs 独立保留——**已定案：并入（三面板随 M1d 删除）**。
3. server 门面形态：`AppState` 从 RuntimeKernel 切换为（V2 Kernel + LibSQLStorage + CognitiveStore）三件套。
   **M1c-server 按此执行（下一轮）**。

## 6. M1c 进度与已知限制（2026-08-16）

- ✅ **CLI 已切换**（commit 93d3e00）：`oaip agent run` 走 V2 AgentLoop（PiEngine + runner），
  `--via-workers` 走 V2 Thinker；trace 落库 wb_trace_event、run 状态写 wb_run、
  evolution 快照经 CognitiveStore 持久化；真实 pi 实测端到端跑通。
- ✅ **server 已切换**（commit f58772f）：POST /run 走 spawn_blocking + run_agent
  （resolver 经 mpsc 通道对接 HTTP 端点 + 超时看门狗 Expire）；approvals 端点改 pending
  注册表；serve 签名换 V2 三件套；v0_3_approval 5 例重写为 V2 HTTP 契约全绿。
- ✅ **M1d 物理删除**（commit b75e7b0）：planner/action/agent/tool_backend/runtime 五门面
  删除；evolution.rs + V1 演化存储 + 6 张 V1 表删除（M2 删除半场提前完成，consumers 归零），
  schema 8→9 + DROP 迁移；/memory /tasks /evolution 端点 + 审计/撤销端点 + 三面板删除
  （Governance 面板改事实轨说明）；v0_3_agent/policy/evolution + bridge_v2 删除，
  v0_1/v0_2 事件断言改显式落库；TraceEvent 归 trace.rs、AppState kernel→storage +
  event_tx 替 EventBus。Rust 324/0/3 + UI tsc+vite+21 vitest 全绿。
- ⏭️ **M2 剩余**：审批审计 V2 查询入口（事实轨 → 端点），其余（三套 Learn 归一）已由
  M1d 的 evolution.rs 删除完成。
- ⚠️ **已知限制（Phase 5 语义投影范畴，非 M1 范围）**：storage 世界对象
  （wb_ontology_object）尚未投影进 V2 Kernel 关系——CLI 的 V2 loop 观察不到 V1 演示数据
  （实测表现为前置条件对空世界判 Deny）。runtime-kernel-v2 Phase 5 落地语义投影时解决；
  期间 V2 测试以 kernel 内关系为世界（r1-r5 契约已钉）。

## 修订日志

| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-16 | 初稿：C1-C6 覆盖确认 + G1-G10 差集 + M1d 删除清单 |
| V1.1 | lushaohui | 2026-08-18 | 状态栏更新：M1 全里程碑关闭（M1a–M1d + G1–G10 全部钉绿），原「待 M1b 契约测试」 stale 标注作废 |
