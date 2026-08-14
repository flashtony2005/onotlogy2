# Evolution 自进化（Phase 6，设计定稿）

| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-03-V6-EVOLUTION |
| 状态 | **设计定稿**（评审拍板） |
| 定位 | Learning Layer 与 Authority Layer 的架构隔离 |

## 0. 核心不变量（置顶）

> **Evolution learns from execution, but learning never becomes authority.**
> 演化可以学习执行结果，但学习永远不会自动成为权力。

三个不等式（直接写进代码与测试）：

```
Pattern ≠ Permission
Pattern ≠ Policy
Pattern ≠ Capability
```

## 1. 一句话交付标准

> Agent 的多次执行经验自动提炼为模式；**SuccessPattern** 经 Proposal → Validation →
> EvolutionGate 受控替代一次人工审批（默认关闭、可审计、可撤销）；**FailurePattern**
> 作为风险信号注入 AgentView 影响决策质量；任何学习产物都不修改
> Policy / Ontology / Capability。

## 2. 架构公式（双通道）

```
Experience
    ↓
Pattern
 ┌───────────────┐
 ▼               ▼
Success       Failure
 ▼               ▼
Proposal      RiskSignal
 ▼               ▼
Validation    AgentView
 ▼               ▼
EvolutionGate  Cognition
 └───────┬───────┘
         ▼
      Decision
         │
         ▼
       Effect
```

**通道隔离（绝不交叉）：**

```
Success → Permission Candidate     （唯一可替代审批的通道）
Failure → Cognitive Warning        （永不进入 Policy / Deny / Capability / Permission）
```

## 3. 决策定稿

| # | 决策 | 结论 |
|---|------|------|
| D1 | 经验信号源 | V2 Memory（record hits）升级：Fiber outcome 埋点——Success / Failed / Compensated / Denied **分别落数**（`loop_impl` 采集） |
| D2 | Pattern 类型分离 | `Pattern = SuccessPattern \| FailurePattern`，**类型层面**分离产物：SuccessPattern 仅可产生 Proposal；FailurePattern 仅可产生 RiskSignal |
| D3 | FailurePattern 进认知层 | RiskSignal → `AgentView.evolution` → CognitiveEngine → Decision（**必须**，非仅计数） |
| D4 | EvolutionGate | V2-native 语义复刻（**不是 V1 代码/数据模型搬运**）：7 条件 AND；失败 = WAIT 非 DENY |
| D5 | 撤销语义 | `EvolutionPermissionState` 五态：Disabled / Candidate / Validated / Enabled / **Revoked**；Revoked 永久禁止 Gate 放行 |

## 4. D4 — EvolutionGate（V2-native）

V1 的语义可以复刻，V1 的代码与数据模型**不复刻**：

```
V1 evolution.rs     ✗ 不依赖
V2 evolution2          ├── Memory（经验）
                       ├── FiberOutcome（信号）
                       ├── Pattern（双通道）
                       └── EvolutionGate（权限替代闸门）
```

**门条件（AND Gate，7 条件全部满足才放行）：**

```
EvolutionGate = Enabled
              ∧ Validated
              ∧ Confidence ≥ 0.8
              ∧ SuccessCount ≥ 3
              ∧ FailureCount = 0
              ∧ Compensable
              ∧ ¬Irreversible
```

`SuccessCount ≥ 3` 是**必要条件，不是充分条件**——绝不是
`success >= 3 → auto approve`。

**流程：**

```
RequireApproval
      │
      ▼
EvolutionGate
      │
 ┌────┴────┐
 │         │
PASS      FAIL
 │         │
 ▼         ▼
Auto      Waiting
Approve   Human Approval
```

**失败语义（写入文档与代码注释）：**

> EvolutionGate failure means "cannot substitute approval", not "permission denied".
> Gate 失败 = 不能替代人工审批 → 回退 Human Approval（WAIT），**不是拒绝执行**。

计数语义：`FailureCount` 统计 Fiber **Failed** 结果；**Compensated** 单独计数
（进入 FailurePattern 风险信号），不参与 Gate 公式。

## 5. D5 — EvolutionPermissionState

```
Disabled    —— 当前未开启（默认）
Candidate   —— 已提出 Proposal
Validated   —— 通过验证
Enabled     —— 已启用（可 Gate 放行）
Revoked     —— 曾获得 Evolution Permission，但已撤销
```

**Revoked ≠ Disabled**：Disabled 只是"当前没开启"；Revoked 表示"曾经获得过
Evolution Permission 但已被撤销"——**此后即使重新满足全部条件也永不 Gate 放行**
（防止 `enabled = false → 重新满足 → 自动 enabled = true` 的越界复活）。

MVP：内部不做完整状态机转移，但**保留该语义**（枚举 + 每动作一条
`learned.auto_approve_enabled` 对应 Revoked 标记）。

## 6. Invariants（架构级，代码与测试共同保证）

| # | 不变量 |
|---|--------|
| I1 | SuccessPattern 本身**不能直接产生 Permission**；必须 Proposal → Validation → Gate |
| I2 | FailurePattern **不修改 Policy**、不 Deny、不 disable Capability、不永久禁止 Gate |
| I3 | Gate 失败 = 不能替代审批 → 回退人工（WAIT），不是权限拒绝（DENY） |
| I4 | Revoked 后永不自动恢复 Enabled |

## 7. 测试规划（E1–E8 + 架构级反向测试）

| 测试 | 验证 |
|------|------|
| E1 | Fiber outcome 正确采集（Success/Failed/Compensated/Denied 分账） |
| E2 | Success / Failure / Compensation Pattern 提炼（阈值 + 幂等 id） |
| E3 | SuccessPattern → Proposal（仅此通道可产生） |
| E4 | FailurePattern → AgentView.evolution（RiskSignal 注入） |
| E5 | EvolutionGate 七条件 AND 判断（任一缺失 → WAIT 非 DENY） |
| E6 | Gate 通过 → 审计事实（`evolution.gate_approved` 事件） |
| E7 | **Revoked 后永久不能自动 Gate**（红线验证，必须有） |
| E8 | End-to-end：经验 → Pattern → Cognitive/Permission → Agent 闭环 |

**架构级反向测试：**

- `FailurePattern → AgentView → Decision → Policy`：验证 Policy 未被修改
- `SuccessPattern → EvolutionGate`：验证 Pattern 本身不能直接产生 Permission
  （必须经 Proposal → Validation → Gate）

## 8. Non-goals

- ❌ 不修改 Policy / Ontology / Capability（红线）
- ❌ 不自动生成新 Capability / 新 ODL
- ❌ 演化状态持久化（认知状态，P4 不进 fact 层；并入 Memory 持久化延期项）
- ❌ 时间窗口衰减算法（MVP 简单计数）
- ❌ LLM / embedding 聚类

## 9. 实现规划

| 步骤 | 内容 | 文件 |
|------|------|------|
| 1 | 经验模型 + Pattern 双通道类型 + Proposal/RiskSignal + Validation + PermissionState | `src/evolution2/mod.rs` |
| 2 | EvolutionGate（7 条件 AND + WAIT 语义 + 审计调用） | `src/evolution2/gate.rs` |
| 3 | AgentView.evolution 注入 + observe 接线 | `src/cognitive/engine.rs` `observe.rs` |
| 4 | outcome 采集埋点（run_once 四类结果落数） | `src/cognitive/loop_impl.rs` |
| 5 | TaskExecutor 接 gate（`with_evolution` 注入，RequireApproval 分支） | `src/compose/task.rs` |
| 6 | 测试 E1–E8 + 反向不变量测试 | `tests/evolution_v2.rs` |
| 7 | 门禁（fmt/clippy/test 全绿）+ 本设计文档提交 | — |
