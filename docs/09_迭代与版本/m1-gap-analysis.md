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

### G1 认知 Trace 事件流（最大缺口）

- V1：run_goal 全链路 emit 12 种认知事件（Goal / Intent / OntologyQuery / MemoryRecall /
  Reasoning / Planning / PolicyCheck / ToolBackend / ActionFailed / Action / MemoryUpdate /
  Reflection）→ `wb_trace_event(kind=cognitive)` → UI 时间线 + WS 流 + evaluation 桥接。
- V2：`run_once` 零事件。切换后 UI 时间线空白、`v0_9_ws_clamp` 失效。
- 补齐：AgentLoop 各阶段 emit 对等认知事件（最小集 = UI `buildTimeline` 实际消费的种类 +
  evaluation 桥接所需）；事件写入经统一入口，保留 run_id/object_refs 绑定。
- 测试：`tests/v2_parity/trace_events.rs`——断言 run_until_done 全程事件序列（种类/顺序/绑定）。

### G2 审批适配器（CLI + HTTP 两条腿）

- V1：`PolicyApprover` 三实现——`ConsoleApprover`（stdin y/N + 决策缓存）、`AutoApprover`、
  `ServerApprover`（挂起 + `GET /run/:id/approvals` + `POST /run/:id/approval/:req_id` + 超时 Expired）。
- V2：只有 `Waiting` + `relate(Approval)` 机制，无外部适配器。
- 补齐：① `CliApprovalAdapter`（stdin 交互，复用 V1 决策缓存语义）；② `HttpApprovalAdapter`
  （复用现有端点协议，待审队列改由 Waiting Fiber 注册表驱动，超时→advance(Expired 语义)）。
- 测试：`tests/v2_parity/approval_cli.rs` / `approval_http.rs`——挂起→批准恢复→完成；
  挂起→拒绝→Denied；挂起→超时→Expired 且拒绝二次裁决。

### G3 Pi 真实引擎迁移

- V1：`AgentEngine` trait + `PiAgentEngine`（agent.rs 子进程管理 + 超时 kill+wait）。
- V2：`CognitiveEngine` trait 只有 `FakeEngine` / `NoOpEngine`，Pi 适配口未实现。
- 补齐：`PiAgentEngine` 子进程封装迁移为 `cognitive/pi.rs::PiEngine: CognitiveEngine`
  （decide = 复用 V1 intent 分类 + 能力选择 + 决策生成的 prompt 编排）；
  `AgentEngine` trait 与 `WorldSnapshot` 随 planner 删除。
- 测试：`tests/v2_parity/pi_engine.rs`——进程健康检查、超时回收、decide 形状（复用 v0_3 的 Fake 断言模式，真实 Pi 用例保持 ignore）。

### G4 审批审计落库（REQ-018 可审计承诺）

- V1：`wb_approval_audit` 表 + `GET /evolution/audit` + AutoApprove 可撤销 API。
- V2：`step_with_gate` 放行不写任何审计。
- 补齐：gate 放行与 relate 审批两个路径都落审计事件（表结构归 M2 统一：建议并入
  Session Log 事实轨而非独立表——待 M2 定案，此处先保证「有审计」）。
- 测试：`tests/v2_parity/approval_audit.rs`——每次 gate 放行/人工审批均有可回放审计记录。

### G5 「无失败」数据源切换

- V1：`count_action_failed` 从 trace 表统计 `ActionFailed` 事件喂给 AutoApprove 闸门。
- V2：`EvolutionEngine` 内存态分账（Success/Failed/Denied/Compensated），未持久化。
- 补齐：gate 的 failure 判定改读 evolution2 分账；分账经 `CognitiveStore` save/restore
  （配合 C4，Phase 10 已承诺）。
- 测试：`tests/v2_parity/gate_failure_source.rs`——失败历史经持久化后跨 run 生效，
  与 V1 count_action_failed 语义对齐（限定键 `(type, action)`）。

### G6 run 状态映射与持久化

- V1：`RunRecord{status, human_approval}` + `wb_run` 表 + 重启恢复；UI `deriveState` 依赖。
- V2：Fiber 状态机（CREATED→…→COMPLETED/FAILED/COMPENSATED/ESCALATED）。
- 补齐：`FiberState → RunStatus` 纯函数 + run 元数据落库（run_id/goal/agent_id/状态），
  供 health 与 UI 使用。
- 测试：`tests/v2_parity/run_status.rs`——全状态映射表 + 持久化往返。

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
