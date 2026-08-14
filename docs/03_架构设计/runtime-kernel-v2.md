# Runtime Kernel V2.0 — Spatiotemporal Ontology Runtime

| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-03-V2-KERNEL |
| 状态 | 设计定稿（2026-08-14） |
| 前置 | architecture-total.md（V1 分层）、Cordis disposable+hierarchy 借鉴 |

## 0. 一句话

OpenAIP-OS V2.0 的核心不再是「语义对象」，而是四个 Primitive：

```
Context（我在哪里存在）
Fiber（谁正在经历时间）
Effect（它改变了什么）
Relation（谁和谁发生了什么）
```

其它一切（Service / Capability / Policy / Task / Agent / World Model）都是从这四个 Primitive **向上生长**的组合层，不是 Kernel Primitive。

## 1. 四条 Architecture Principles（定稿）

### P1 — Failure Compensation

> Fiber failure automatically triggers reverse-order compensation for committed compensatable effects. Compensation failure escalates to human intervention.

- Fiber 失败 → 对已提交 Effect 按**逆序**执行补偿（C⁻¹ → B⁻¹ → A⁻¹）
- 统一叫 **Compensation**，不叫 rollback（现实世界很多操作无法真正回滚）
- 补偿失败 → 状态 `ESCALATED` → 人工介入，同时落事实（`Fiber --has_state--> CompensationFailed`、`Effect B --compensation_failed--> true`）全程可审计

### P2 — Lifecycle Isolation

> Context disposal terminates its fibers and releases runtime resources, but never implicitly compensates business effects.

- `Context.dispose()` = 终止子 Fiber + 释放资源，**不自动补偿**
- 理由：Session 结束 ≠ 刚创建的订单应删除；**Context 生命周期 ≠ 业务事务生命周期**
- 业务需要补偿 → 上层显式发起（Task Cancel → Compensate → Dispose，或 Supervisor 决定）

### P3 — Temporal Minimalism

> Kernel guarantees only current-valid state. Historical reconstruction and as_of queries belong to the Temporal World Model phase.

- Kernel 只保证 `relation.is_current(now)`（valid_from/valid_until 过滤）
- `as_of(t)` / replay / diff / event sourcing 全部推迟到 Phase 4（World Model 层）
- 守住边界：Kernel 不膨胀成 Temporal Database Engine

### P4 — Cognitive Separation

> LLM reasoning is a Fiber poll operation and belongs to Agent Cognitive State, not the Fact Layer. Only committed Effects may mutate World Facts.

- LLM 推理 = Fiber 的 `poll()` 步骤，**不进入 fact 层**
- `CognitiveOutput { Observation, Hypothesis, Decision }` 属于 Agent 认知状态
- 只有 `Decision → Capability → Policy → Effect` 才改变 World

## 2. Kernel Algebra

```
Runtime State = (Context, Fiber, Effect, Relation)
State(t) --Effect--> State(t+1)
```

最小 API（Kernel 只暴露这五个动作）：

```rust
trait Runtime {
    fn create_context(&self, parent: Option<ContextId>) -> ContextId;
    fn spawn(&self, context: ContextId, task: FiberTask) -> FiberId;
    fn emit(&self, fiber: FiberId, effect: Effect) -> EffectId;
    fn relate(&self, relation: Relation) -> RelationId;
    fn dispose(&self, fiber: FiberId);   // Context 销毁为 dispose_context
}
```

## 3. Context — 时空作用域

```rust
struct Context {
    id: ContextId,
    parent: Option<ContextId>,
    entity_id: String,          // 同一 Entity 不同时间段的 Context 靠它定位（D4 补充）
    scope: Scope,               // WORLD / TENANT / SESSION / AGENT / TASK / ACTION
    valid_from: Timestamp,
    valid_until: Option<Timestamp>,
    state: StateStore,
    fibers: FiberRegistry,
    effects: EffectRegistry,
}
```

Context 状态机（与 Fiber 完全分离）：

```
ACTIVE → DISPOSING → DISPOSED
```

层级（权限继承天然成立）：

```
World → Tenant → Session → Agent → Task → Action
Policy 逐层继承（比独立 RBAC 更强）
```

## 4. Fiber — 世界中的生命体

```rust
struct Fiber {
    id: FiberId,
    context: ContextId,
    parent: Option<FiberId>,
    state: FiberState,
    input: Value,
    output: Option<Value>,
    ledger: EffectLedger,       // 已提交 Effect 账本（补偿依据，Fiber 自己的责任）
    relations: Vec<RelationId>,
    created_at: Timestamp,
}
```

**Fiber 状态机（最终锁定版）**：

```
CREATED → STARTING → READY → RUNNING ⇄ WAITING / PAUSED
                                    │
                        ┌───────────┴───────────┐
                        ↓                       ↓
                    COMPLETED                FAILED
                                                ↓
                                          COMPENSATING
                                          /          \
                                         ↓            ↓
                                   COMPENSATED    ESCALATED（人工介入）
```

- `WAITING` = 等待恢复信号（审批 / 定时器 / 外部事件，恢复统一经 `relate`）
- 补偿是 **Fiber 自己的责任**（持有 EffectLedger，失败自身进入 COMPENSATING），Runtime 不猜测怎么回滚

## 5. Effect — 可审计副作用 + 补偿

```rust
pub enum Reversibility {
    Reversible,      // DB transaction → 可直接撤销
    Compensatable,   // Create Order → 发反向业务操作
    Idempotent,      // Send API request → 重放安全
    Irreversible,    // Send Email → 不可逆（Policy 可要求审批）
}

struct Effect {
    id: EffectId,
    fiber: FiberId,
    action: Action,
    input: Value,
    output: Option<Value>,
    reversibility: Reversibility,
    inverse: Option<Compensation>,   // 补偿定义（可能产生新的 Effect）
    status: EffectState,
    timestamp: Timestamp,
}
```

- `emit(Effect)` 原子完成：执行副作用 + 写入 Relation 投影 + 写 Event Log（同事务；跨资源副作用由补偿链兜底）
- `relate()` 仅用于**外部世界事件**的进入点（如 `Approval --approved_by--> User`）
- 可逆性直接衔接现有 `require_approval`：`effect.irreversible ⇒ policy 可要求审批`

## 6. 认知边界（P4 的三层世界）

```
        COGNITION          AGENT
   Observation / Reasoning / Hypothesis / Decision
                          │  Decision
                          ↓
        RUNTIME           Context → Fiber → Effect → Compensation
                          │  Effect
                          ↓
        WORLD             Entity / Relation / State / Fact
```

- 推理观察 → 现有 `wb_trace_event(kind=cognitive)` 继续承载（观察层）
- 事实（Effect/Relation）→ 新 `effects` / `relations` 表（fact 层）
- **观察层与事实层分离**

## 7. Phase 路线（定稿）

| Phase | 内容 | 验收 |
|-------|------|------|
| 0 | Context / Fiber / Effect / Relation 四 Primitive + 状态机 | kernel 单测全绿 |
| 1 | Scheduler / Lifecycle / Effect Ledger / Compensation / Persistence | 删除用户闭环端到端 |
| 2 | Service / Capability / Policy / Task（组合层） | 声明式动作经 Fiber 执行 |
| 3 | Planner / Agent / Memory / Approval / Human-in-the-loop | 现有 run_goal 迁移到 Fiber 模型 |
| 4 | Temporal World（as_of / replay / diff / event sourcing） | 时间旅行查询 |
| 5 | Ontology Compiler / GraphRAG / World Model | 语义投影 |
| 6 | Self-Evolving Agent | 闭环自治 |

## 8. 与现有 V0.9 的映射（迁移策略）

| V2.0 Primitive | 现有实现 | 迁移动作 |
|---|---|---|
| Context | `run_id` + `agent_id` + 角色 | 加 entity_id/valid 区间/层级（新增表） |
| Fiber | `run_goal` 实例 + `RunStatus` | run_goal 重构为 Fiber poll 循环（Phase 3） |
| Effect | `ActionEngine` 副作用 + `TraceEvent` | 加 inverse/reversibility/ledger（新增表） |
| Relation | `wb_relation_edge` | 加 context/valid_from/valid_until（增量列） |
| 审批挂起 | `ServerApprover` + oneshot | 统一为 Fiber WAITING + relate 恢复（同构，零破坏） |
| 认知事件 | `wb_trace_event(kind=cognitive)` | 保留为观察层，事实层新增 events 表 |

原则：**新 kernel/ 模块并行生长，旧 planner 不推翻，Phase 3 逐步迁移**。
