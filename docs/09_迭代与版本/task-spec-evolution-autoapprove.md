# Task Spec：Evolution 第二阶段 —— LearnedKnowledge 受控自动放行（AutoApprove 闸门）

| 项 | 值 |
|----|----|
| TaskID | WB-TASK-018 |
| 版本 | V0.6（已实现） |
| REQ | REQ-018 |
| Test | TC-026a ~ TC-026h |
| 状态 | 已实现（TC-026a~h 全绿，门禁 fmt/clippy/test 全绿，已提交） |

## 0. 设计红线（不可动摇）
**第一原则 Learn ≠ Permission 仍然成立，本 Task 是它被显式、受控、可撤销地放开，而非废弃。**

- MVP（REQ-017）中 `LearnedKnowledge` 只可见、不改权限。本 Task 让「满足全部闸门的 `LearnedKnowledge`」可在 `require_approval` 动作上**自动放行**（替代一次 Human Approval）。
- 任何一条闸门不满足 → **退回 MVP 行为**（仍需 Human Approval），绝不降级为"放行"。
- **默认关闭**：`RuntimeConfig.auto_approve_from_evolution = false`；不显式开启，行为等同 MVP。
- **可审计**：每次自动放行写入 `wb_approval_audit`（谁/何时/基于哪条 LearnedKnowledge/哪次 run/结果）。
- **可撤销**：管理员可在 Evolution UI / API 撤销某条 LearnedKnowledge 的自动放行资格；撤销后该 action 立即退回 Human Approval。
- **不修改 Policy 语义**：`require_approval` 字段含义不变；AutoApprove 是"在满足闸门时**代替** approver 返回 true"，不是删除 `require_approval`。

## 1. Capability（能力）
在满足 5 道受控闸门的前提下，允许基于已验证的 `LearnedKnowledge` 对 `require_approval` 动作**自动放行**（AutoApprove），从而减少重复 Human Approval 负担；闸门不满足或默认关闭时，严格退回 MVP 的 Human Approval 行为，且不改变 `Policy` 对任何动作的 Deny 判定。

## 2. Boundary（边界）
### 做
- 新增 `auto_approve_from_evolution` 全局开关（默认 false）。
- 在 `Policy::check_action` 的 `require_approval` 分支（planner.rs:431）插入「Evolution 自动放行判定」：
  - 仅当开关开启 + 该 action 存在已 Validated 的 `LearnedKnowledge` + 5 道闸门全过 → 替代 `approver.approve()` 返回 `Allow`。
  - 任一条件不满足 → 仍走原 `approver.approve()`（Human Approval）。
- 新增 `ActionFailed` 认知事件（见 TC-026f 埋点），支撑「无失败」闸门强判定。
- 新增 `wb_approval_audit` 表，记录每次自动放行（可追溯至 LearnedKnowledge + run）。
- 新增 `Evolution UI` 的「自动放行资格」展示与撤销入口（后端提供撤销 API）。
- 新增 REQ-018 + TC-026a~h 验收矩阵。

### 不做（显式排除）
- **不**修改 `Policy` 的 precondition / `require_approval` 语义，也不删除任何 Deny 路径。
- **不**让 AutoApprove 对 `require_approval = false` 的动作产生任何影响（那些动作本就 Allow）。
- **不**引入 LLM 参与 AutoApprove 判定（确定性 + 闸门，可解释）。
- **不**把 AutoApprove 做成"学习即生效"——必须显式开开关 + 全闸门 + 已 Validated。
- **不**进入 Kernel 核心安全路径之外——AutoApprove 仅是 `PolicyApprover` 上游的一个"可替代放行"判定，失败即回退。
- 第二阶段**不做**基于 ODL `risk_level` 字段的判定（已决议采用现有 PolicyDef 弱判定）。
- 第二阶段**不做**时间窗口衰减、多 action 关联学习等高级信号（MVP 已知边界，留待后续）。

## 3. Definition（定义）
### 3.1 自动放行判定函数（纯函数，便于单测）
```rust
/// 5 道闸门全过 + 开关开启 + 存在已 Validated LearnedKnowledge → 允许自动放行
fn may_auto_approve(
    config: &RuntimeConfig,        // auto_approve_from_evolution: bool
    learned: Option<&LearnedKnowledge>,  // 该 action 的已沉淀知识（须 status=Validated）
    success_runs: usize,           // 该 action 成功执行的去重 run 数（= learned.run_ids.len()）
    failure_count: usize,          // 该 action 的 ActionFailed 事件数（新增埋点）
    policy: &ActionPolicy,         // require_approval / precondition
) -> bool
```
**5 道闸门（全部满足才 true）：**
1. **显式允许**：`config.auto_approve_from_evolution == true`。
2. **已验证知识**：`learned.is_some() && learned.status == Validated`。
3. **高置信**：`learned.confidence >= AUTO_APPROVE_CONFIDENCE`（取 `VALIDATE_THRESHOLD` 之上，建议 `0.8`，见 TC-026c 边界）。
4. **多次成功**：`success_runs >= AUTO_APPROVE_MIN_RUNS`（建议 `3`，即至少 3 个去重 run 成功执行过该 action）。
5. **无失败**：`failure_count == 0`（依赖新增 `ActionFailed` 埋点，强判定）。
6. **低风险**：`policy.require_approval == true && !policy.precondition.is_empty()`（已有人工审核门口 + 前置护栏；决议采用现有 PolicyDef 判定）。

> 注：门槛常量（0.8 / 3）初值，落到 `src/evolution.rs` 或 `src/planner.rs` 顶部，命名 `AUTO_APPROVE_*`，经 TC-026c/d 锁定。

### 3.2 数据结构变更
- `RuntimeConfig` 新增字段 `auto_approve_from_evolution: bool`（默认 false）。
- `wb_approval_audit` 表（新增）：
  ```
  id TEXT PK, run_id TEXT, action TEXT, learned_id TEXT,
  decision TEXT,  -- "auto_approve" | "fallback_human"
  reason TEXT, created_at TEXT
  ```
- `LearnedKnowledge` 新增 `auto_approve_enabled: bool`（默认 true；撤销即置 false，仍可观察但不再自动放行）。
- 新增认知事件类型 `ActionFailed`（trace.rs `COGNITIVE_STEPS` 不动，仅作为 Action 失败时的 `output` 区分或新增 event_type，待实现定；TC-026f 锁定）。

### 3.3 撤销 API
- `POST /api/v1/wb/agent/evolution/learned/:id/auto-approve`（`enabled=true|false`）——幂等，仅翻转 `auto_approve_enabled` + 记 audit。
- 不删除 LearnedKnowledge，仅切换自动放行资格。

### 3.4 错误 / 回退
- 任何异常（审计写失败、learned 读取失败）→ **回退 Human Approval**，不静默放行（TC-026g）。
- 撤销后下次该 action 命中 → `may_auto_approve` 因 `auto_approve_enabled==false` 返回 false → 回退 Human Approval。

## 4. Method（方法）
- 技术选型：复用现有 `LearnedKnowledge`（evolution.rs）+ `Policy::check_action`（planner.rs:406）+ `RuntimeConfig`；新增 `ActionFailed` 埋点（planner.rs:797 执行后失败分支）+ `wb_approval_audit` 存储与读取。
- 实现步骤：
  1. 加 `ActionFailed` 埋点 + `wb_approval_audit` 表（codegen + storage）。
  2. `RuntimeConfig` 加开关；`LearnedKnowledge` 加 `auto_approve_enabled`。
  3. 纯函数 `may_auto_approve`（evolution.rs 或 planner.rs）。
  4. `Policy::check_action` 插入自动放行判定（上游替代 approver，失败回退）。
  5. 撤销 API + Evolution UI 资格展示。
- 依赖：REQ-017（LearnedKnowledge / run_ids 可追溯）、REQ-015（Policy 闸门位置）。
- **TDD 节奏（Red 先行）**：
  - 先写 **TC-026a**（默认关闭时 `require_approval` 动作仍 Deny→WB2005，行为等同 MVP，不回归）—— Red；
  - 再写 **TC-026e**（开启 + 全闸门过 → 自动放行 Allow），**TC-026b**（开关 false 强制回退 Human），**TC-026c**（置信不足回退），**TC-026d**（成功次数不足回退），**TC-026f**（有 ActionFailed → 回退），**TC-026g**（异常回退），**TC-026h**（审计 + 撤销可回退）。
  - 最小实现转 Green，重构守护边界（不触碰 Kernel 安全路径）。

## 5. 完成判定（DoD）
- TC-026a~h 全绿（含默认关闭不回归、各闸门单点回退、异常回退、审计+撤销）。
- `cargo fmt` / `clippy -D warnings` / `tsc --noEmit` / `vite build` 全绿。
- 代码评审：确认未修改 `Policy` Deny 语义、未删除 `require_approval`、AutoApprove 仅作"可替代放行"。
- 文档同步：REQ-018 登记、test-case TC-026、architecture/接口文档补 `wb_approval_audit` 与撤销 API、Evolution UI 视图补资格展示。

## 6. 风险点 & 注意事项
- **最大风险**：AutoApprove 变相"学习即放行"。闸门设计即为此兜底；TC-026b/c/d/f 必须显式验证每个闸门缺失都回退 Human。
- **埋点遗漏**：`ActionFailed` 若只在部分失败路径发，会导致"无失败"闸门误判放行。TC-026f 必须覆盖失败路径全埋点。
- **回退安全**：任何异常必须回退 Human，绝不"放行失败"。TC-026g 锁死。
- **可解释性**：每次自动放行必写 audit（可追溯至 LearnedKnowledge + run），否则违反 OpenAIP 可解释根基（见 trace 规则）。

## 7. 关联文档
- [requirement-all.md](./requirement-all.md)（REQ-018 登记）
- [test-case.md](./../07_测试文档/test-case.md)（TC-026a~h）
- [todo-backlog.md](./todo-backlog.md)（BL-003 第二阶段）
- [evolution.rs](./../src/evolution.rs)（LearnedKnowledge / 闸门常量）
- [planner.rs](./../src/planner.rs)（Policy::check_action 接入点）

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V0.1 | lushaohui | 2026-08-12 | 初稿契约：5 道闸门 + 默认关闭 + 可审计 + 可撤销；登记 REQ-018；TC-026a~h 矩阵；决策：ActionFailed 强判定 + 现有 PolicyDef 弱判定低风险 |
| V0.2 | lushaohui | 2026-08-12 | 落地：状态 待认领→已实现；`may_auto_approve` 纯函数 + `RuntimeConfig.auto_approve_from_evolution` 默认关闭；`Policy::check_action` 接入（上游替代 approver，异常回退 Human）；`wb_approval_audit` + `LearnedKnowledge.auto_approve_enabled` 可撤销 + `ActionFailed` 埋点；`--auto-approve-evolution` + `POST /evolution/learned/:id/auto-approve` 撤销 API；TC-026a~h 全绿 |
