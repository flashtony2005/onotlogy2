# Domain Agent Evolution Runtime（Phase 11 · V11.0 Final，设计定稿 + Implementation Spec）

| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-03-V11.0-DOMAIN-AGENT-EVOLUTION |
| 状态 | **设计定稿 V11.0 Final**（评审拍板，正式冻结为 Final Design Contract） |
| 关系 | 在 Phase 9（Skill Asset Compiler & Registry）与 Phase 10（Universal Worker & Plugin Runtime + Session Event Log）之上的**进化层**；V10.0/V10.1 文档继续有效 |
| 定位 | 只解决一个问题：**如何让 Agent 在特定领域持续进化**——不训练基础模型、不修改模型参数；进化的是 Context Policy / Skill Asset / Router Prior |

## 0. 一句话定义

> OpenAIP-OS 不进化模型，进化资产。由 Context、Execution、Evaluation、Memory、Mutation
> 构建**确定性** Evolution Loop，使 Agent 在特定领域越做越好——
> 模型固定，资产在进化，Runtime 在学习，Event Log 保证一切可解释、可回放、可验证。

## 1. 核心命题

```
Context → Skill → Capability → Worker → Execution → Event Log
  → Evaluation → Experience → Mutation → Benchmark → Promotion
  → New Context / Skill / Router → Next Run
```

即：**Observe → Evaluate → Learn → Mutate → Validate → Promote → Execute**。

Worker 只是执行器。真正让系统产生"智能增长"的，是
Context → Execution → Evaluation → Memory → Mutation → 再执行 的闭环。
"持续进化"不是重新训练模型，而是一个**可观测、可测评、可回放**的 Agent Learning Loop。

## 2. 第一原则：Evaluation = Pure Function(Event Log)

```
Evaluation(events) → metrics
```

必须满足：

| 性质 | 含义 |
|------|------|
| Deterministic | 同一事件集 E 永远得到 Evaluation(E) = S，重跑结果不变 |
| Idempotent | 重复调用 evaluate(E) 结果完全一致 |
| Replayable | 删除 evaluation 结果后：Event Log → Reducer → Evaluation 可完整重建 |

因此：**Evaluation Result 是派生数据，不是事实。**

## 3. Fact 与 Judgment 彻底分离（第二条铁律）

```
Event Log   = Fact
Evaluation  = Judgment
```

- 事实：`worker.started / worker.completed / worker.failed / worker.selected / skill.selected / context.assembled`
- 判断：`correctness = 0.91 / output_quality = 0.84`

Judge 流程：Human Judge / LLM Judge → `judge.submitted` → Event Log → Evaluation Reducer。
**Judge 本身也成为可审计事实**（携带 rubric_version + confidence），绝不混入事实层。

## 4. 六维 Evaluation 模型

| 维度 | 类型 | 来源 |
|------|------|------|
| correctness | 半客观 | expected_relations + judge |
| completeness | 半客观 | 产物/事件链 + judge |
| efficiency | 客观 | 时间 / cost |
| reliability | 客观 | failed / retry |
| policy_compliance | 客观 | approval 链 |
| output_quality | 主观 | judge.submitted |

示例结果：

```json
{
  "correctness": 0.91,
  "completeness": 0.88,
  "efficiency": 0.76,
  "reliability": 0.95,
  "policy_compliance": 1.0,
  "output_quality": 0.86
}
```

注意：**综合分数只是派生视图，底层六维指标必须保留。**

客观维度（efficiency / reliability / policy_compliance）绝不用 LLM 推断；
主观/半客观维度的 judge 修正一律经 `judge.submitted` 事件进入，带 rubric_version + confidence。

## 5. Evaluation Reducer

```
EvaluationReducer( SessionEvent[], TaskDefinition, EvaluationPolicy ) → EvaluationResult
```

内部结构（子 reducer 可独立扩展）：

```
EvaluationReducer
├── CorrectnessReducer
├── CompletenessReducer
├── EfficiencyReducer
├── ReliabilityReducer
├── PolicyComplianceReducer
└── OutputQualityReducer
```

以后增加 `maintainability / security / cost_efficiency` 只加子 reducer，不破坏整个 Evaluation 系统。

## 6. 新增事实事件（Phase 11 只新增两个）

V10.1 已拥有 `worker.selected / worker.started / worker.completed / worker.failed / skill.selected / plugin.call / plugin.result / policy.checked / approval.*` ——全部复用。

Phase 11 只新增：

**context.assembled**（记录本次运行选择了哪些 Context Asset，不是 Context 完整内容）：

```json
{
  "kind": "context.assembled",
  "run_id": "run_123",
  "assets": [
    { "asset_id": "java.architecture", "selection_reason": "project_type=java", "relevance": 0.91 },
    { "asset_id": "integration.testing", "selection_reason": "task.requires_testing", "relevance": 0.87 }
  ]
}
```

解决关键问题：**为什么这一次成功/失败？哪些上下文参与了决策？**

**judge.submitted**（判断信号，可审计）：

```json
{
  "judge": "human",
  "dimension": "output_quality",
  "score": 0.86,
  "rubric_version": "rubric.v1",
  "confidence": 0.9
}
```

于是 Execution Facts + Context Fact + Judgment Fact 足以构建整个 Evolution Loop。

## 7. ContextAsset（Context 进化的事实单元）

Phase 11 **不引入 Vector DB / RAG**（维持 Phase 3 原则）。

```yaml
id: integration-testing
source_ref: knowledge/testing.md
selection:
  project.type: java
  task.requires: integration_test
relevance: 0.87
```

Context Evolution 链（本阶段只打通前三环，相关性挖掘归第二阶段）：

```
Run → context.assembled → Evaluation → Correlation → Context Policy Candidate
```

## 8. Skill Mutation

**Skill 永远不允许原地修改**（`fix-java-bug@v1` 产生 `fix-java-bug@v2`）。
Mutation 必须是**结构化 Delta**：

```json
{
  "kind": "skill_mutation",
  "target": "fix-java-bug@v1",
  "root_cause": "integration test omitted",
  "evidence": ["run_123", "run_124"],
  "delta": {
    "insert_step": { "after": "unit_test", "step": "integration_test" },
    "modify": { "step": "verify", "precondition": "regression_check" }
  },
  "revalidate": ["schema", "semantic", "ontology", "policy"]
}
```

`revalidate` 复用 Phase 9 Validation 四层（Schema / Semantic / Ontology / Policy）。

## 9. Mutation 不是执行结果

Mutation 是 **Candidate Asset**：

```
Skill v1 → Mutation Proposal → Skill v2 Candidate → Validation → Benchmark
```

而不是 `Skill v1 → AI 修改 → Skill v2`。这一区别是安全边界（Phase 8 "绝不做自动代码修改"的延续）。

## 10. Evolution Benchmark（配对实验）

每个候选版本必须做**配对实验**：

- 基线 v1 ≥ 5 runs，候选 v2 ≥ 5 runs
- **同一任务集**（T1..T5 双方各跑一遍），避免任务难度差异造成假提升

## 11. 晋升规则（T11）

v2 必须**同时**满足：

| 规则 | 公式 |
|------|------|
| 效果量 | mean(v2) ≥ mean(v1) + 0.05 |
| 失败率不劣 | failure_rate(v2) ≤ failure_rate(v1) |
| 无新增失败模式 | new_failure_modes(v2) = 0 |

全部通过 → **EXPERIMENTAL**。

## 12. EXPERIMENTAL ≠ ACTIVE（双阶段晋升）

```
Candidate → Validation → Benchmark → EXPERIMENTAL
  → Human Approval / REQ-018 闸门 → ACTIVE
```

- 晋升 EXPERIMENTAL：系统自动（受控小流量可路由）
- 晋升 ACTIVE：Human Approval，或 REQ-018 五道闸门（**默认 OFF**）
- 系统不会因为一次实验成功就自动改变生产行为

## 13. 失败必须保留

v2 未达标 → **REJECTED**，不能删除——**失败本身就是 Experience**。

Registry 最终形成多版本共存：

```
Skill
├── v1 ACTIVE
├── v2 REJECTED
├── v3 EXPERIMENTAL
└── v4 ARCHIVED
```

完整可追溯（与 Phase 9 G11 Fork 语义、Phase 7 version/stale 机制同构）。

## 14. 三种 Evolution

```
Agent Evolution
├── Skill Evolution      —— 学习：怎么做
├── Context Evolution    —— 学习：应该知道什么
└── Router Prior         —— 学习：应该让谁做
```

**Router 绝对边界**：

```
Policy Filter → Candidate Workers → Learned Score → Selection
```

Learned Score 只能在 Policy Filter 之后生效；**绝不能** `Learned Score → Policy`。

## 15. Learn ≠ Permission（Phase 11 架构原则）

```
Skill       → 可以进化
Context     → 可以进化
Router      → 可以进化（仅 Score 统计先验）
Evaluation  → 只读
Policy      → 不进化
```

`allow / deny / approval / permission / secret access` 全部属于 **Immutable Policy Boundary**。

## 16. wb_evaluation（派生结果表）

```
wb_evaluation
├── id                 -- eval.<run_id>（幂等 upsert）
├── run_id
├── task_id
├── skill_version
├── correctness
├── completeness
├── efficiency
├── reliability
├── policy_compliance
├── output_quality
├── aggregate_score
├── evaluator_version
└── created_at
```

**不能把它当事实源——Event Log 永远是 Source of Truth。** 删表可经 Reducer 重建。

## 17. wb_mutation（提案 + Benchmark 持久化）

```
wb_mutation
├── id                 -- mutation.<target>.<candidate_version>（幂等）
├── target_asset
├── source_version
├── candidate_version
├── root_cause
├── evidence           -- run_ids JSON
├── delta              -- 结构化 Delta JSON
├── validation_result
├── benchmark_result
├── status
├── promotion_reason
└── created_at
```

状态机：`PROPOSED → VALIDATED → BENCHMARKING → EXPERIMENTAL → ACTIVE`
失败路径：`→ REJECTED`（保留）；终止：`→ ARCHIVED`。

## 18. Phase 9 / 10 / 11 关系（终于完整）

```
Phase 9  Skill Intelligence      第三方 Skill → AI Parse → IR → Validation → Skill Registry
Phase 10 Universal Worker        Skill → Capability → Worker Router → Codex/Claude/CLI/MCP/API
                                 → Execution → Session Event Log
Phase 11 Agent Evolution         Event Log → Evaluation → Experience → Mutation
                                 → Benchmark → Promotion → Skill/Context/Router → Next Run
```

- Phase 9：让系统能够**理解并获得能力**
- Phase 10：让系统能够**调用现实世界的能力**
- Phase 11：让系统能够**从执行结果中学习，并在受控条件下改进自己的能力**

模型是固定的，资产在进化，Runtime 在学习，Event Log 保证一切可解释、可回放、可验证。

## 19. 与既有路线的衔接（命名调整）

- 原 Phase 11 定名「Software Engineering Ontology」→ **降级为第一个领域实例**（在 Evolution Loop 之上建立 Requirement → Project → Code → Test → Issue → Change → Review → Release 本体）。
- 原 Phase 12（Skill Evaluation & Learning）/ Phase 13（Skill Evolution）→ **并入本 Phase**（六维 Evaluation + Mutation Benchmark）。
- Phase 14（Self-Evolving Agent）→ Evolution Loop 的自主化收束，保持后续展望。

## 20. MVP 实现范围与顺序（本阶段）

1. **先**：`context.assembled` / `judge.submitted` 事件 → Evaluation Reducer → wb_evaluation
2. **再**：Mutation Benchmark（wb_mutation + 配对实验 + 晋升规则）
3. **不做**：UI 页面；复杂"自进化 Agent"；Context 相关性挖掘（第二阶段）；Router 先验学习（第二阶段）；模型训练/微调

## 21. 差距清单（与现有代码映射）

| # | 差距 | 内容 | 复用 |
|---|------|------|------|
| G1 | 新增两事件 | ContextAssembled / JudgeSubmitted | src/session/event.rs 扩展 |
| G2 | Evaluation Reducer | 六维纯函数 + TaskDefinition + EvaluationPolicy | 新模块 src/evaluation/ |
| G3 | wb_evaluation 表 | 派生结果表（可重建） | canonical_schema 模式 |
| G4 | Mutation 引擎 | 结构化 Delta 提案 + 状态机 + 配对 Benchmark + 晋升判定 | evolution2 纯函数模式 |
| G5 | wb_mutation 表 | 提案 / Benchmark 结果持久化 | canonical_schema 模式 |

## 22. 实现规划

| 步骤 | 内容 | 文件 |
|------|------|------|
| 1 | 事件扩展（context.assembled / judge.submitted） | `src/session/event.rs` |
| 2 | Evaluation 引擎（六维纯函数 + 幂等 + 可重建） | `src/evaluation/mod.rs` |
| 3 | canonical_schema 加 wb_evaluation / wb_mutation（含迁移） | `src/codegen.rs` / `src/storage/mod.rs` |
| 4 | Mutation 引擎（提案 + 配对 Benchmark + 晋升规则） | `src/mutation/mod.rs` |
| 5 | 存储（保存/查询，派生表语义） | `src/storage/evaluation.rs` |
| 6 | 集成测试 TC-EV1.. / TC-MU1.. | `tests/evaluation_v2.rs` / `tests/mutation_v2.rs` |
| 7 | 门禁（fmt/clippy/test 全绿）+ 提交 | — |

## 23. 变更记录

| 版本 | 作者 | 日期 | 说明 |
|------|------|------|------|
| V1.0 | lushaohui | 2026-08-15 | 评审冻结 Phase 11 Final Design Contract：Evaluation = Pure Function(Event Log)、六维模型、Fact/Judgment 分离、Mutation Candidate + 配对 Benchmark、EXPERIMENTAL≠ACTIVE 双阶段、Learn≠Permission、wb_evaluation / wb_mutation 两表；同步 Phase 9/10/11 路线衔接（SE Ontology 降为第一个领域实例） |
| V1.1 | lushaohui | 2026-08-15 | **Phase 11A 冻结（Executable Feedback Loop）**：目标从「Self-Evolving Agent」重定义为「运行中的 Feedback Loop」；Evaluation 升级为**证据系统**（Score{value,evidence,confidence} + unknowns，Unknown≠Success）；TestResult 成为 correctness 一等证据；policy_compliance 无证据→None；RunEnd 钩子（on_run_end → 落 wb_evaluation → 发射 evaluation.created）接入 CLI 生产路径；context.assembled 由 run 边界发射；新增 GET /api/v1/wb/agent/evaluation；硬原则：不再增加「未来可参与进化」的组件，直到现有组件已进入 Event → Evaluation → Diagnosis → Replay 数据流 |
