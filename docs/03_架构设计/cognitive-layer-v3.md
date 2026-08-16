> [废弃] M7（2026-08-16）：本文件描述 V0.x/V2 中间态设计；当前架构以 `architecture-v2.md` 为准。

# Agent 认知层 V3.0 — 设计（Phase 3，未实现）

| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-03-V3-COGNITIVE |
| 状态 | **设计冻结**（2026-08-14 评审通过；D1/D2/D3/D5 ✅，D4 按评审调整） |
| 前置 | runtime-kernel-v2.md（Phase 0-2 已落地）、P4 Cognitive Separation |

## 0. 目标

让 Agent 在 V2.0 Runtime 上真正"活"起来：

```
Agent（长期 Fiber）
  └── Task Fiber（每次任务一个实例）
        └── poll 循环：Observe → Reason → Decide → Capability → Policy → Effect → Observe Result
```

- **认知层（observe/reason/decide/memory）在 Kernel 之外**（P4：不进 fact 层）
- 只有 `Decision → Capability → Effect` 改变 World
- 现有 V1 `run_goal` 继续可用（并行路径），本阶段不迁移、不推翻

## 1. 五个设计决策

### D1 — Agent 是长期 Fiber，Task 是子 Fiber

```
Agent Context（agent.cfo）
  └── Agent Fiber（长期存活，poll = Agent Loop）
        └── Task Fiber #1（"分析本月销售"）
        └── Task Fiber #2（…）
```

- Agent = 长期 Fiber + Memory + 决策引擎 + 能力视图
- 每次任务 = Task 声明 + spawn 一个 Task Fiber（设计 §6/§7 落地）
- Task Fiber 失败补偿不影响 Agent Fiber 存活（P2：Context 级生命周期隔离）

### D2 — 认知产出三分（P4 落地，评审冻结 ✅）

```rust
pub enum CognitiveOutput {
    Observation(Observation),  // 世界事实的观察（只读）
    Hypothesis(Hypothesis),   // 推断（不修改世界）
    Decision(Decision),       // 决策：执行哪个能力
}
```

**Fact 准入表（认知不是事实）**：

| 类型 | 进入 Fact 层 |
|------|-------------|
| Observation | ❌ |
| Hypothesis | ❌（绝不把猜测污染成世界事实） |
| Decision | ❌ |
| Effect（已提交） | ✅ |
| Effect Result（世界确认） | ✅ |

- Observation/Hypothesis 进 Agent Memory / Trace 观察层
- 只有 Decision 进入 `TaskExecutor::step` → Effect

### D3 — Memory 是 Agent 认知状态，不是 fact 层（评审冻结 ✅）

- Memory = `HashMap<label, { payload, hits }>`（V1 wb_memory 语义映射）
- 沉淀时机：Effect 成功提交 → `memory.record(action)`（hits++）
- 召回时机：Decision 前注入相关记忆（MemoryRecall 语义）
- **Phase 3 只做内存态 TaskContext**（不引入数据库/向量库/RAG）：

```rust
pub struct TaskContext {
    pub world: WorldSnapshot,       // 观察快照
    pub memory: Memory,             // 认知状态
    pub history: Vec<CognitiveOutput>, // DecisionHistory（observations/hypotheses/decisions/outcomes）
}
```

- 持久化：Phase 4（Temporal World）统一处理；本阶段坚持克制，避免膨胀成
  Cognitive Runtime + Knowledge Base + RAG

### D4 — CognitiveEngine 抽象（评审调整 ⚠️ → ✅）

保留抽象，明确职责边界；**MVP 内部用确定性 FakeEngine，LLM 仅 Adapter**——
以后接真实 LLM 不修改 Agent Loop。

**MVP 职责（本阶段）**：

```
CognitiveEngine
├── Intent Recognition      （复用 V1 intent.rs：classify_intent_text）
├── Parameter Extraction    （复用 V1 intent.rs：extract_action_args）
├── Capability Selection    （world + memory + goal → 候选能力）
└── Decision Generation     （capability + args + confidence）
```

**未来职责（不实现）**：Rule Engine / LLM Adapter / World Model / Memory / Planning / Decision Policy

- V1 `run_goal` 的编排职责 → V2 由 Agent Loop（Fiber poll）承担（run_goal 本阶段不迁移）
- 决策引擎 = 确定性实现优先（闭环可测），Pi/Claude 只实现同一 trait

### D5 — expected_relations = 目标函数（评审冻结 ✅，Phase 3 最关键的决策）

**Agent 不信任自己的执行——重新观察世界确认目标达成。**

```
Goal: "激活用户 Mason"
Expected Relation: User(Mason) --status--> active

Agent 不因 activate_user() 执行成功就宣布完成；
而是重新 observe 世界，确认 User(Mason) --status--> active 真的存在。
```

- Task 增补 `expected_relations: Vec<(predicate, target)>`
- Agent Loop 每轮观察后检查：期望关系全部存在 → `advance(Completed)`
- 这使 Agent 从 **Command Executor**（执行了 = 完成）变为
  **Goal-directed Runtime**（世界达成了 = 完成）——也是未来 World Model 的基础

**D2 + D5 组合 = Observe → Decide → Act → Verify**（而非传统 Think → Tool Call → Done）

## 1.5 决策确认（评审结论）

| # | 决策 | 结论 |
|---|------|------|
| D1 | Agent 长期 Fiber / Task 子 Fiber（MVP 不做多任务并发） | ✅ |
| D2 | Observation / Hypothesis / Decision 三分（认知≠事实） | ✅ |
| D3 | Memory 只做运行时认知状态（TaskContext 内存态），Phase 4 再持久化 | ✅ |
| D4 | 保留 CognitiveEngine 抽象，FakeEngine 实现，LLM 仅 Adapter | ⚠️→✅（职责分层） |
| D5 | expected_relations 作为唯一完成判定（目标函数） | ✅ |

## 2. 模块结构

```
src/cognitive/
├── mod.rs        # 聚合：CognitiveOutput / Observation / Hypothesis / Decision / Agent / AgentLoop
├── memory.rs     # Memory（record / recall）
├── engine.rs     # CognitiveEngine trait + FakeEngine（确定性）+ PiEngine 适配口
├── observe.rs    # 观察：kernel relations/state → AgentView（世界快照 + 相关关系 + 记忆）
└── loop.rs       # AgentLoop：run_once / run_until_done
```

compose/ 增补（Phase 2 移交项）：
- `capability.rs`：+ `required_params: Vec<String>`（② schema 校验：step 前必填检查）
- `task.rs`：+ `expected_relations` + `TaskExecutor::complete(fiber, task)`（① 完成判定）
- `service.rs`：+ `bind(&mut kernel, ctx)`（③ 记录 `Service --serves--> Context` 关系）

## 3. 核心 API

```rust
// 认知引擎（可替换：Fake / Pi / Claude）
pub trait CognitiveEngine: Send + Sync {
    fn decide(&self, view: &AgentView) -> Result<Option<Decision>, CognitiveError>;
}

pub struct AgentView {
    pub goal: String,
    pub world: Value,              // 观察快照（observe 构建）
    pub related: Vec<(String, String, String)>,  // (source, predicate, target)
    pub memory: Vec<MemoryItem>,   // 召回记忆
    pub capabilities: Vec<String>, // 可用能力（Service 提供）
}

pub struct Decision {
    pub capability: String,
    pub args: Value,
    pub confidence: f64,
}

// Agent Loop（驱动 Task Fiber 的 poll）
pub struct AgentLoop<'k> {
    kernel: &'k mut Kernel,
    agent: &'k Agent,
    services: &'k [Service],
}

impl<'k> AgentLoop<'k> {
    /// 一轮：Observe → Reason → Decide → step（Policy/审批/Effect/投影）
    pub fn run_once(&mut self, fiber: FiberId, task: &Task) -> Result<LoopOutcome, KernelError>;
    /// 完整循环：直到 Completed / Denied / Waiting / Failed
    pub fn run_until_done(&mut self, ctx: ContextId, task: &Task, input: Value) -> Result<LoopOutcome, KernelError>;
}

pub enum LoopOutcome {
    Completed,          // expected_relations 满足或无更多决策
    Waiting,            // 审批挂起（relate 恢复后可继续 run_until_done）
    Denied(String),     // 业务拒绝（已补偿）
    Failed(String),     // 运行失败（已补偿或升级人工）
}
```

## 4. Agent Loop 流程（每轮 poll）

```
poll(fiber):
  observe()            → AgentView（世界快照 + 相关关系 + 记忆召回）
  engine.decide(view)  → Option<Decision>
    None               → 检查 expected_relations 满足？→ Completed : Failed("无决策且未达成")
    Some(d)            → TaskExecutor::step(fiber, cap(d.capability), d.args)
                          → Committed  → memory.record(action)；continue
                          → Waiting    → 暂停（LoopOutcome::Waiting）
                          → Denied     → LoopOutcome::Denied（已 FAILED→空补偿）
  effect 失败 → fiber FAILED → CompensationEngine → Compensated/Escalated
```

- **Waiting 恢复**：外部 `relate(Approval)` → 自动 resume → 调用方再 `run_until_done` 续跑
- **Memory 沉淀**：Committed 后 `memory.record(capability.action)`
- **Human-in-the-loop**：即 Waiting 暂停 + relate 恢复（Phase 1/2 机制，零新增）

## 5. 测试契约（tests/agent_v2.rs）

| # | 场景 | 断言 |
|---|------|------|
| A1 | Agent 闭环（FakeEngine）：world 有 Customer 001 → 决策 retain → Effect + 投影 → expected_relations 满足 → COMPLETED | 状态流转 + relations + memory |
| A2 | 审批暂停恢复：决策 refund（require_approval）→ Waiting → relate → 续跑完成 | LoopOutcome::Waiting → Completed |
| A3 | Memory 沉淀/召回：成功执行后 record；下一轮决策前 view.memory 含该项 | hits 累加 + 注入决策 |
| A4 | Deny（前置条件）→ FAILED → 空补偿 → LoopOutcome::Denied | 无 Effect + 事件 |
| A5 | expected_relations 不满足且无决策 → Failed（不误判完成） | 状态断言 |
| A6 | Service 绑定 Context（③）：bind 后 relation `Service --serves--> Context` | 关系断言 |
| A7 | required_params 校验（②）：缺参 → Deny | 事件 + 状态 |

## 6. 边界（不做）

- **不迁移 run_goal**：V1 与 V2 并行，Phase 3 末尾评估迁移时机
- **Pi 决策引擎**：只留 trait 适配口，不接真实 LLM（FakeEngine 闭环验收）
- **Memory 持久化**：Phase 4
- **Fiber Tree 调度**（Agent Fiber 抢占/时间片）：后续阶段
- **Agent 多任务并发**：Phase 3 单任务循环

## 7. 验收标准

1. A1-A7 全绿（143 + ~10 新测试）
2. Agent 闭环在 **kernel 五动作 + TaskExecutor** 上跑通，认知层零侵入 kernel
3. 审批 Human-in-the-loop 全程可审计（events 事实层）
4. V1 全部测试保持绿（并行路径证明）
