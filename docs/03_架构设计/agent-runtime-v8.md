# Agent Runtime Production Kernel（Phase 8，设计定稿 + Implementation Spec）

| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-03-V8-AGENT-RUNTIME |
| 状态 | **设计定稿 + Implementation Spec**（评审拍板） |
| 定位 | 前 7 个 Phase 的**收束**——生产级闭环 Agent Runtime 最小落点，不是无限增加功能 |

## 0. 北极星（置顶，以后所有设计讨论以此为准）

> **Phase 8 构建一个以 Fiber 为执行原语、以 Capability 为能力原语、以 Policy 为安全边界、
> 以 Event 为事实来源、以 Compensation 为失败语义、以 Agent 为决策层的可恢复 Agent Runtime。**

```
                    ┌─────────────┐
                    │    Agent    │
                    │ Goal / Plan │
                    └──────┬──────┘
                           ↓
                    ┌─────────────┐
                    │   Policy    │
                    └──────┬──────┘
                           ↓
                    ┌─────────────┐
                    │    Fiber    │
                    └──────┬──────┘
                           ↓
                    ┌─────────────┐
                    │ Capability  │
                    └──────┬──────┘
                           ↓
                    ┌─────────────┐
                    │    World    │
                    └──────┬──────┘
                           ↓
                    ┌─────────────┐
                    │    Event    │
                    └──────┬──────┘
                 ┌─────────┴─────────┐
                 ↓                   ↓
              Success             Failure
                 ↓                   ↓
             Complete          Compensation
                 │                   │
                 └─────────┬─────────┘
                           ↓
                         Audit → Episode → Learning → Candidate Skill
```

## 1. 一句话价值主张

只证明一件事：**一个 Agent 是否能在真实业务环境中，以"可计划、可授权、可暂停、
可恢复、可补偿、可审计"的方式执行复杂任务。**

## 2. 不做什么（这次定稿最重要的部分）

- ❌ 不做通用 AGI / 自己的大模型 / 复杂多 Agent 社会
- ❌ 不做无限 Ontology / 完整 Palantir 替代品
- ❌ 不做自动修改自身代码的"黑盒自进化"
- ❌ 不继续无止境增加 DSL；不把所有能力塞进 Runtime

## 3. 核心闭环与 8 概念

```
User Goal → Goal Parser → Planner → Plan → Policy Engine → Execution Graph
  → Fiber Runtime → Capability → External World → Event → State → Decision
  → Continue / Retry / Compensate / Abort
```

| 概念 | 语义 | V2 现状 |
|------|------|---------|
| Plan | 我要做什么 | ✅ planner + compose::Task |
| Policy | 允许不允许做 | ✅ compose::policy（与 Planner 分离） |
| Capability | 能够做什么 | ✅ compose::Capability + runtime_exec |
| Fiber | 正在做什么 | ✅ kernel::Fiber（11 态） |
| Event | 发生了什么 | ✅ kernel events + world event_log |
| State | 现在是什么状态 | ✅ kernel relations + world as_of |
| Compensation | 失败以后怎么办 | ✅ 逆序补偿 + Escalated |
| Audit | 为什么这样做 | ✅ KernelEvent + adoption_log + gate_approved |

## 4. 最终架构（7 层）

```
Agent Layer      Goal / Intent / Memory / Learning        ✅ cognitive / evolution2 / rule
Planning Layer   Planner / Plan / DAG / Replanning        ✅ planner
Policy Layer     Auth / Approval / Constraint / Guard     ✅ compose::policy + auth
Runtime Layer    Fiber / Scheduler / Context / Recovery   ✅ kernel（Recovery 补 Interrupted）
Capability Layer Tool / Skill / Action / Connector        ✅ compose + runtime_exec
State Layer      Event / Snapshot / Journal / Audit       ✅ kernel events + persistence
Storage          Turso / SQLite                           ✅ storage
```

**真正的核心只有：Runtime + State + Capability + Policy——Agent/LLM 均可替换。**

## 5. 已定稿的既有语义（沿用 V2，不重做）

| 主题 | 定稿 | V2 现状 |
|------|------|---------|
| Context 生命周期 | `dispose()` 只销毁 Runtime Context，**不自动补偿**；业务补偿由上层显式 `compensate(fiber)`（资源生命周期 ≠ 业务事务生命周期） | ✅ P2 已实现 + K5 测试 |
| Compensation | Fiber 失败 → 自动逆序 → 补偿失败 → **Manual Intervention**；每个补偿动作产生 Event | ✅ K2/K3/K4（Escalated ≈ ManualIntervention；事件类型补 `compensation.failed` 对齐命名） |
| Approval | `require_approval` 是 **Runtime Suspension Point**（非 Tool）：Fiber 挂 WaitingApproval；Approve → Resume；**Reject → Abort/Compensation** | ⚠️ 挂起/恢复 ✅（wait_on + relate）；**Reject 路径 ❌ 缺失（无 Cancelled 状态）** |
| Policy Engine | `authorized` 与 Planner 彻底分离：Planner 提建议、Policy 决定能不能做、Runtime 执行决定 | ✅ 已分离（PolicyVerdict Allow/Deny/RequireApproval） |
| Capability 统一 | Tool/Skill/Action/Function/Command 统一为 **Capability**（Agent 选择 Capability，不是调用工具） | ✅ compose::Capability |
| Event 事实来源 | Event + Snapshot（replay 可得 Current State）→ 审计/Debug/Recovery/时间旅行 | ✅ world replay + kernel persistence |
| Memory | **Execution Memory**（Goal/Plan/Action/Result/Failure/Compensation/Approval）是 Event 的上层消费者；不做"聊天记忆" | ⚠️ 认知 Memory（认知状态）；Execution Memory = 从 Event 提炼（S10 增量） |
| Self-Evolving | **Learning → Candidate → Evaluation → Human Approval → Skill Registry**；绝不做自动代码修改 | ✅ Phase 6/7 管线（Improvement Proposal 而非自动改） |

## 6. Implementation Spec（真正的增量——差距清单）

### 差距总览

| # | 差距 | 对应 Scenario | 现状 |
|---|------|--------------|------|
| G1 | **Idempotency key**：同一 key 执行两次不产生两次业务效果 | S8 | ❌ 仅 Reversibility::Idempotent 分类，无执行去重 |
| G2 | **Approval Reject → Cancelled**：Reject 后 Abort/补偿 | S4 | ❌ 无 Cancelled 状态、无 reject 路径 |
| G3 | **Crash Recovery 显式化**：Interrupted 状态 + 恢复后 resume | S7 | ⚠️ 快照持久化 ✅；Interrupted 语义 ❌ |
| G4 | **ManualIntervention 事件语义**：补偿失败必须产生事件，不能吞异常 | S6 | ⚠️ Escalated ✅；事件命名/可见性补齐 |
| G5 | **Execution Memory / Episode**：从 Event 提炼 Episode → Pattern → Improvement Proposal | S10 | ⚠️ evolution2 管线 ✅；Episode 提炼 ❌ |
| G6 | **S1-S10 场景验收测试**：10 个 Scenario 全部可执行可断言 | 全部 | ❌ 部分分散（S3≈A2/E8，S5≈K2，S6≈K3） |

### G1 — Idempotency（执行去重）

```
Capability/Effect 携带 idempotency_key（调用方提供或按 args 哈希派生）
执行前：kernel 查重（context + key）→ 已执行 → 返回原结果（不产生新 Effect/关系）
执行后：记录 (context, key → effect_id)
```

- `Kernel::execute_idempotent(fiber, capability, args, key)` / TaskExecutor step 接 key
- 事件：`effect.idempotent_replay`（审计可见：重放而非新执行）
- 测试：同 key 两次 step → 世界只变化一次

### G2 — Approval Reject → Cancelled

```
Fiber 状态机新增：Cancelled（终态）
审批 Reject：advance(Waiting → Cancelled) + 事件 approval.rejected + 逆序补偿（如有已提交 Effect）
```

- `Kernel::reject_approval(fiber, by)` + 状态机转移 (Waiting → Cancelled) + 补偿收尾
- 事件：`approval.requested` / `approval.granted` / `approval.rejected`（对齐 §7 事件全集）
- 测试：S4 Reject → Cancelled + 补偿；Cancelled 后不可 resume

### G3 — Crash Recovery

```
Fiber 状态机新增：Interrupted（崩溃恢复中间态）
恢复：restore 快照 → Interrupted 的 fiber → 上层决定 resume（Running）或补偿（Failed→补偿）
```

- 持久化恢复时：Running（崩溃）→ Interrupted（V1 v0_8 语义移植到 V2 kernel）
- 事件：`fiber.interrupted` / `fiber.recovered`
- 测试：S7 快照 → 恢复 → resume → 完成（replay 校验）

### G4 — ManualIntervention 事件语义

- Escalated 已存在；补齐事件：`compensation.started` / `compensation.completed` / `compensation.failed` / `manual_intervention_required`
- 测试：S6 补偿失败 → Escalated + 事件可见（不吞异常）

### G5 — Execution Memory / Episode → Improvement Proposal

```
Event Journal → Episode（goal/plan/action/result/failure/compensation/approval 聚合）
Episode → Pattern（复用 evolution2 提炼）→ Improvement Proposal（仅提案，Human Approval 后才进 Skill Registry）
```

- `src/cognitive/episode.rs`：从 kernel events 聚合 Episode
- 复用 Phase 6 管线：Episode → experience → mine → propose（**输出 Proposal，不自动改生产**）
- 测试：S10 同种失败多次 → Improvement Proposal 产生

### G6 — S1-S10 场景验收

| Scenario | 内容 | 测试 |
|----------|------|------|
| S1 | 普通任务：List 所有用户（Goal→Plan→Policy→Capability→Result） | `tc_s1_*` |
| S2 | 多步骤：CreateUser→SendEmail（Fiber 内多 capability 顺序执行） | `tc_s2_*` |
| S3 | Approval：DeleteUser→Waiting→Approve→Resume→Complete | `tc_s3_*`（A2/E8 已有模式） |
| S4 | Reject：DeleteUser→Reject→Cancel | `tc_s4_*`（G2） |
| S5 | Failure：A→B→C FAILED→逆序补偿 | `tc_s5_*`（K2 已有模式） |
| S6 | Compensation Failure→ManualInterventionRequired | `tc_s6_*`（G4） |
| S7 | Crash Recovery：Event Replay→Recover→Resume | `tc_s7_*`（G3） |
| S8 | Idempotency：同 key 两次执行无二次效果 | `tc_s8_*`（G1） |
| S9 | Audit：任何执行可回答 谁/为什么/计划/授权/调用/结果/失败原因/补偿 | `tc_s9_*`（事件全链路断言） |
| S10 | Learning：同种失败多次→Improvement Proposal（非自动改） | `tc_s10_*`（G5） |

## 7. Event 类型全集（对齐命名）

```
FiberCreated / PlanCreated / PolicyChecked / CapabilityStarted / CapabilityCompleted
ApprovalRequested / ApprovalGranted / ApprovalRejected
CapabilityFailed / CompensationStarted / CompensationCompleted / CompensationFailed
ManualInterventionRequired / FiberCompleted / FiberInterrupted / FiberRecovered
EffectIdempotentReplay
```

（现有 `effect.committed / relation.created / policy.denied / evolution.gate_approved` 保留）

## 8. Non-goals（本轮明确不做）

- ❌ Retrying 自动重试策略（失败 → 补偿优先；重试是上层决策，仅保留状态语义空间）
- ❌ Cancelled 之外的终态扩展
- ❌ Capability output_schema / postconditions（required_params 已覆盖 MVP；schema 化留 Phase 9）
- ❌ 大目录重构（AGENTS.md：不保留向后兼容性 ≠ 无谓搬移——7 层收敛是**概念映射**，现有模块结构不动）
- ❌ 数据库 schema 变更（沿用 MIGRATIONS 框架；本轮无新表，事件/状态均在现有 kernel 结构内）
- ❌ 自动代码修改 / 自动 Skill 上线

## 9. 实现规划

| 步骤 | 内容 | 文件 |
|------|------|------|
| 1 | G1 Idempotency：kernel 执行去重 + `effect.idempotent_replay` 事件 | `src/kernel/` |
| 2 | G2 Reject：Cancelled 状态 + `reject_approval` + approval.* 事件 | `src/kernel/fiber.rs` `mod.rs` |
| 3 | G3 Recovery：Interrupted 状态 + 恢复语义 + 事件 | `src/kernel/fiber.rs` `persistence.rs` |
| 4 | G4 补偿事件全集（compensation.* / manual_intervention_required） | `src/kernel/` `compose/task.rs` |
| 5 | G5 Episode：Event→Episode→Pattern→Improvement Proposal | `src/cognitive/episode.rs` |
| 6 | G6 S1-S10 场景验收测试 | `tests/runtime_v2.rs` |
| 7 | 门禁（fmt/clippy/test 全绿）+ 提交 | — |

## 10. 数据模型闭环（定稿确认）

```
Actor → Goal → Plan → Policy → Fiber → Capability → Event → Episode → Skill
（意图 → 计划 → 约束 → 执行 → 世界反馈 → 经验 → 能力增长）
```
