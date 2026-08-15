# Self-Evolving Agent（Phase 7，设计定稿）

| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-03-V7-SELF-EVOLVING |
| 状态 | **设计定稿**（评审拍板） |
| 定位 | V2.0 → Self-Evolving Runtime 的第一步：Agent 的决策机制成为自身可观察、可评价、可受控采用的对象 |

## 0. 第一原则（置顶）

> **Self-Evolution must follow the same execution path as ordinary world evolution.**
> 自我进化不能拥有一条特殊的"后门路径"；它必须沿着系统已经验证过的
> Observe → Decide → Act → Verify → Learn → Evolve 路径发生。

行为演化改变的是"**Decision 如何产生**"，而不是"Decision 如何执行"。

## 1. 分形迭代（为什么 Phase 7 不是"加一个模块"）

同一个 `Observe → Decide → Act → Verify → Learn → Evolve` 结构在不同尺度递归：

```
WORLD → OBSERVE → COGNITION → DECISION → AUTHORITY → EFFECT
  → WORLD CHANGE → EXPERIENCE → PATTERN → BEHAVIOR PROPOSAL
  → VALIDATION → ADOPTION → RULESTORE CHANGE → NEW COGNITION → OBSERVE …
```

| 层 | 路径 | 状态 |
|----|------|------|
| ① Agent → World | Ontology → Capability → Cognitive → Decision → Effect → World → Observation | ✅ V2.0 Phase 0–5 已完成 |
| ② Agent → Agent | Experience → Pattern → EvolutionProposal → Validation → Adoption → New Behavior | ✅ 本阶段（Phase 6 管线 + 本设计） |
| ③ Agent → Ontology | … → OntologyProposal → Validation → Approval → Ontology Evolution → 重新编译 → 新 Runtime | 🔭 Phase 8 展望（ABI 预留） |

## 2. 嵌套治理（分形的稳定性条件）

```
        Authority
           ▲
           │ governs（Validation/Approval/Effect）
       Evolution
           ▲
           │ proposes
       Cognition
           ▲
           │ observes
         World
```

**双层治理：**

- **Cognitive Evolution**：RuleStore → RuleEngine → Decision 可自我改变（自动 Adoption）；
- **Authority Evolution**：Policy / Capability / Ontology / Authority 不能被该路径改变。

> 认知层可以自主演化认知结构；权力层仍然保持独立治理。
> 低层产生 Proposal，高层决定是否 Adoption——**不是任何东西可以修改任何东西**。

Phase 6 的 `Learning ≠ Permission` 是这一分形架构的稳定性条件，Phase 7 直接延伸。

## 3. 决策定稿

| # | 决策 | 结论 |
|---|------|------|
| D1 | RuleStore：行为即数据 | ✅ 通过。`BehaviorRule { id, capability, args, condition, enabled, priority }`——Agent 行为从代码逻辑变为可演化数据 |
| D2 | RuleEngine implements CognitiveEngine | ✅ 通过。严格 `AgentLoop → CognitiveEngine::decide() → RuleEngine → Decision → TaskExecutor`，**不做特殊分支** |
| D3 | ConditionExpr 结构化条件 | ✅ 通过。取消闭包：`HasRelation / HasRisk / Not / All / Any`（+ future 扩展位 `HasState`，Phase 7 不扩） |
| D4 | 规则级经验 rule_stats | ✅ 通过。`Decision.rule_id → Outcome → rule_stats`，与 action 级分账并存——**两个尺度的经验** |
| D5 | BehaviorProposal：Disable / Promote | ✅ 通过，**增加 target_version / snapshot_hash**（并发演化一致性） |
| — | Adoption 人工审批 | ❌ MVP 不增加。**无人工审批，但不是无治理**：Validation 就是 Cognition Layer 的自动治理闸门 |
| — | Validation 治理验证 | ✅ 升级为治理不变量（见 §5，9 条） |
| — | Authority 修改 | ❌ 严格禁止（Promote 不改变 Policy/Capability/Approval/Effect 语义） |
| — | World 路径 | ✅ 完全复用 V2 Kernel（Effect/补偿/审批/Verify 零 bypass） |
| — | EnableRule | ❌ 保持 Non-goal |

## 4. D1–D5 细节

### 4.1 D1 — RuleStore（行为 ABI）

与 RuntimeDefinition 同构的可序列化结构，未来形成三尺度"可描述结构"：

```
BehaviorRule         （本阶段）
RuntimeDefinition    （Phase 5）
OntologyDefinition   （Phase 8 展望）
```

```rust
pub struct BehaviorRule {
    pub id: String,            // 稳定 id
    pub capability: String,    // 能力名（如 "customer.retain"）
    pub args: Value,           // 决策参数
    pub condition: ConditionExpr,
    pub enabled: bool,         // false = 已停用（可被 Disable 提案）
    pub priority: i64,         // 选择顺序（可被 Promote 提案提升）
    pub version: u64,          // 每次 adopt 递增——并发演化一致性
}
```

### 4.2 D3 — ConditionExpr（Evolution ABI）

闭包可以执行，但不能演化；ConditionExpr 可以
serialize → inspect → compare → validate → propose → mutate → audit → rollback。

```rust
pub enum ConditionExpr {
    HasRelation { predicate: String, target: String },
    HasRisk { action: String },        // 读取 AgentView.evolution（Phase 6 RiskSignal）
    Not(Box<ConditionExpr>),
    All(Vec<ConditionExpr>),
    Any(Vec<ConditionExpr>),
    // HasState{..} —— future 扩展位（Phase 7 不实现）
}
```

### 4.3 D4 — rule_stats（双尺度经验）

```
Action Statistics（Phase 6，action 粒度）+ Rule Statistics（本阶段，rule 粒度）
```

`Decision { capability, args, confidence, rule: Option<rule_id> }` →
outcome 埋点按 rule_id 分账（rule_stats 与 action 级分账并存）。

### 4.4 D5 — BehaviorProposal（带版本一致性）

```rust
pub enum BehaviorChange {
    Disable { rule_id: String },
    Promote { rule_id: String, new_priority: i64 },
}

pub struct BehaviorProposal {
    pub id: String,               // 幂等 id
    pub target_rule_id: String,
    pub expected_version: u64,    // 目标规则版本（stale 防护）
    pub change: BehaviorChange,
    pub evidence: ProposalEvidence,  // rule_stats / confidence / 来源
    pub validation: Vec<ValidationCheck>, // 逐条治理验证记录
    pub status: ProposalStatus,   // Proposed / Validated / Rejected（/ Stale）
}
```

**Stale 防护**：`adopt()` 时 `current_version == expected_version` 才允许；
否则 → **stale proposal → reject（revalidate 后可重提）**。
并发演化下旧 Proposal 不能覆盖新的行为状态。

### 4.5 Adoption 语义（无人工审批，但不是无治理）

```
Proposal → Validation → Adoption     ✅（Self-Evolving Loop 闭合）
Proposal → Adoption                  ❌（跳过治理验证）
Proposal → Validation → WAIT HUMAN → Adoption  ❌（那是 Human-assisted evolution，非本阶段）
```

## 5. Validation — 治理验证不变量（9 条，全部满足才 adopt）

Validation 不是"经验够多了"，而是：**这个行为修改是否满足 Cognition Layer 的安全不变量。**

```
Validation
├── ① Evidence sufficient          （rule_stats 达到证据阈值）
├── ② Confidence sufficient        （置信度 ≥ 阈值）
├── ③ Target version matches       （expected_version == current_version）
├── ④ Capability still exists      （目标能力仍存在）
├── ⑤ Policy unchanged             （Policy 未被该路径修改）
├── ⑥ Ontology unchanged           （Ontology 未被该路径修改）
├── ⑦ At least one valid behavior path remains   （no-dead-agent）
├── ⑧ No irreversible authority change           （Disable/Promote 不触碰 Authority）
└── ⑨ Rollback available           （adoption_log 可撤销）
```

### ⑤⑥⑧ 的工程含义

- **Promote = 改变 Cognition Selection Order，而不是获得更多权限**：
  `Promote` 只改 `priority`，绝不改变 Policy / Capability / Approval requirement / Effect 语义。
- **Authority 修改严格禁止**：RuleEngine / RuleStore 路径不存在任何 Policy / Capability / Ontology 写入接口。

### ⑦ no-dead-agent（F-Rev3）

Disable 不能把 Agent 变成"无路可走"：

```
Rule A → retain
Rule B → retain
Rule C → retain
Disable A + Disable B + Disable C  ❌ Validation 拒绝
```

Validation 必须保证：Disable 后仍存在**有效的 fallback behavior**（或该 Task 可明确
进入 Waiting/Failed），不能产生非法 Decision（无决策引擎）。

### ⑨ Rollback（撤销）

Adoption 记录在 `adoption_log`（审计）：`{ proposal_id, rule_id, old_state, new_state, ts }`；
`undo_adoption(proposal_id)` 恢复旧状态（enabled/priority/version 回退）。

## 6. 测试规划

| 测试 | 验证 |
|------|------|
| F1 | 行为即数据：BehaviorRule / ConditionExpr 序列化 roundtrip |
| F2 | RuleEngine 决策：结构化条件等价闭包语义（含 HasRisk 条件） |
| F3 | 同一路径：RuleEngine 进 AgentLoop 全流程（审批/补偿/Verify 零 bypass） |
| F4 | 规则级经验分账（outcome → rule_stats） |
| F5 | BehaviorProposal 提炼：高频失败 → Disable；高频成功 → Promote |
| F6 | Validation 门槛：经验/置信不足 → 不 adopt |
| F7 | Adoption 后行为改变：停用规则不再被选 / 提升规则优先被选 |
| F8 | 端到端：行为演化闭环 + adoption_log 审计 + 撤销恢复 |
| F-Rev1 | 无后门：未 Validated 的 proposal 无法 adopt；行为变化必有审计记录 |
| F-Rev2 | 不侵 World：行为演化全程 kernel 事件/关系零异常 |
| F-Rev3 | **Disable 安全边界**：Disable 最后一个可行 Rule → Validation reject（no-dead-agent） |
| F-Rev4 | **Proposal Staleness**：Rule v3 出提案 → Rule 变 v4 → Adopt → reject |

## 7. Non-goals

- ❌ EnableRule（重新启用已停用规则需 Ontology 确认，Non-goal）
- ❌ 第三层 Ontology 演化（Phase 8 展望；BehaviorRule/RuntimeDefinition 同构 ABI 已预留）
- ❌ LLM 行为生成
- ❌ RuleStore 持久化（认知状态，并入认知持久化延期项）
- ❌ HasState 条件（future 扩展位）

## 8. 实现规划

| 步骤 | 内容 | 文件 |
|------|------|------|
| 1 | RuleStore + BehaviorRule + ConditionExpr（可序列化 ABI）+ RuleEngine | `src/rule/`（mod/rule.rs, expr.rs, engine.rs） |
| 2 | Decision.rule 字段 + rule_stats 埋点（loop_impl 按 rule 分账） | `src/cognitive/engine.rs` `loop_impl.rs` |
| 3 | BehaviorProposal（Disable/Promote + expected_version）+ 治理 Validation（9 不变量） | `src/rule/evolution.rs` |
| 4 | Adoption：adopt()（version 校验 + 审计 adoption_log）+ undo_adoption | `src/rule/evolution.rs` |
| 5 | 测试 F1–F8 + F-Rev1~4 | `tests/rule_v2.rs` |
| 6 | 门禁（fmt/clippy/test 全绿）+ 本设计文档提交 | — |

## 9. Phase 7 完整分形路径

```
WORLD → OBSERVE → COGNITION → DECISION → AUTHORITY → EFFECT
  → WORLD CHANGE → EXPERIENCE → PATTERN → BEHAVIOR PROPOSAL
  → VALIDATION → ADOPTION → RULESTORE CHANGE → NEW COGNITION → OBSERVE …
```

**核心创新**：让 Agent 的"决策机制"第一次成为它自己可以观察、评价、
提出修改并受控采用的对象。
