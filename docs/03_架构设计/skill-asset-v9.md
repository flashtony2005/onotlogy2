# AI Skill Asset Compiler & Registry（Phase 9，设计定稿 + Implementation Spec）

| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-03-V9-SKILL-ASSET |
| 状态 | **设计定稿 + Implementation Spec**（评审拍板） |
| 定位 | 正式建立 OpenAIP-OS 的 **Skill 编译器 + 能力资产层（Capability Acquisition Layer）** |

## 0. 最终哲学（置顶）

> Phase 8 的问题是"Agent 如何可靠地行动？"；Phase 9 的问题是"**Agent 如何获得能力？**"
>
> 第三方 Skill 不再是文本——它是人类/其他 Agent 对"如何完成某类目标"的**经验编码**。
> AI Compiler 把这种经验从 Natural Language 转化为
> `Ontology + Capability + Workflow + Policy + Artifact + Metrics` → **Skill Asset**。

外部经验 → Skill Import → AI 理解 → Ontology → Capability → Skill Asset
→ Runtime 执行 → 真实反馈 → Experience → Skill Mutation → 新 Skill → Export → 外部世界

## 1. 核心命题与完整闭环

把第三方世界中非标准的 Skill 知识，转换为 OpenAIP-OS 内部**标准化、可执行、可组合、可演化**的 Skill Asset：

```
Third-party Skill → Import → AI Semantic Parse → Skill IR
  → Ontology/Capability/Policy Mapping → Validation → Skill Asset
  → Skill Registry → Compose / Evolve → New Skill → Export
```

## 2. 要解决的问题

1. **Skill 格式不统一**：SKILL.md / Prompt / Markdown / JSON / MCP / Workflow / SOP / README——系统不能要求第三方遵循我们的格式。
2. **Skill 语义没有标准**："分析竞争对手"实际包含 Search → Collect → Compare → Score → Summarize → Recommend——必须由 AI 恢复 `Goal → Workflow → Capability → Artifact → Constraint → Effect`。
3. **Skill 无法直接组合**：A 输出什么？B 接受什么？A 能否接 B？——必须建立 **Machine-readable Skill Contract**。

## 3. 核心设计原则（与既有架构不变量对齐）

| 原则 | 语义 | 对应既有不变量 |
|------|------|----------------|
| P1 Raw 不修改 | 第三方 Skill 永远保留原始快照；任何 AI 转换都是**派生资产** | RuntimeDefinition 只增不改（Phase 5） |
| P2 AI 理解 ≠ 授权 | AI 只做 Semantic Interpretation；Validator/Policy/Registry/Runtime 决定能不能进系统、能不能执行 | **P4 认知分离**（Phase 3）：LLM 推理不进 fact 层 |
| P3 先 IR 再 Asset | 禁止 `Text → Executable`；必须 `Text → SkillIR → Validation → SkillAsset` | IR → RuntimeDefinition 两步编译（Phase 5 D1） |
| P4 Capability 不凭空创造 | 未注册能力只产生 **CapabilityCandidate**（UNRESOLVED），绝不偷建真实 Capability | Learn ≠ Permission（Phase 6）；Pattern ≠ Capability |
| P5 所有 AI 推断可追溯 | Capability/Goal/Policy/Postcondition/Artifact 均追溯到 原始 Skill + Source Span + Interpretation + Confidence | 审计不变量（Phase 8 S9） |

## 4. 核心对象模型（MVP 实现集）

```
RawSkill ──parse──▶ SkillIR ──resolve/validate──▶ SkillAsset ──register──▶ Registry
                        │                              │
                        └──▶ CapabilityCandidate (UNRESOLVED)   └──compose/fork──▶ 新 Asset
```

| 对象 | 定义（MVP） |
|------|-------------|
| RawSkill | 第三方资产只读快照：`{ id, name, source_type, source_uri, content_hash, content, author, license, version, imported_at }`；source_type 支持 Markdown/TXT/JSON/YAML/Prompt/MCP/Workflow（**MVP 仅 Markdown/SKILL.md**） |
| SkillIR | AI 标准语义理解：`{ identity, goal, inputs, outputs, workflow, capabilities, artifacts, preconditions, postconditions, policies, metrics, feedback_loops, constraints, provenance }`；每项携带 `{ confidence, evidence, source_span }` |
| SkillAsset | 通过 Validation 的资产：IR 全部字段 + `{ type, evaluation, status, lineage }` |
| CapabilityCandidate | 未解析能力的占位：`{ id, semantic_action, confidence, status: UNRESOLVED }`——不是真实 Capability |
| SkillType | `Atomic / Composite / Workflow / Strategy / Optimizer`（MVP 至少识别 Atomic/Composite/Workflow） |

## 5. Skill 类型与 Capability Schema

### Skill 类型（MVP 分类）

| 类型 | 语义 | 示例 |
|------|------|------|
| Atomic | 一个核心动作 | GenerateThumbnail |
| Composite | 多个 Skill/Capability 组成 | VideoGeneration |
| Workflow | 有明确执行顺序 | YouTubeChannelSetup |
| Strategy | 偏决策 | ContentGrowthStrategy |
| Optimizer | 根据反馈调整其他 Skill | ContentOptimization |

### Capability Schema（Phase 9 完成 schema 化——Phase 8 Non-goal 的落点）

```
capability:
  id: video.generate
  input_schema / output_schema
  preconditions: [prompt.valid]
  postconditions: [video.created]
  effects: [VideoCreated]
  compensation: { capability: video.delete }
```

Capability = **Contract + Action + Effect**。现有 `compose::Capability`（required_params/projection/inverse/effects）为 Contract 子集；MVP 扩展 `input_schema/output_schema/postconditions` 为可序列化结构（对接 RuntimeDefinition ABI）。

## 6. 三个 Resolver（AI 语义 → V2 资产）

| Resolver | 职责 | 落点 |
|----------|------|------|
| Capability Resolver | `semantic_action`（如 create_video）→ Registry 查找 → `resolved{capability, confidence}` 或 `CapabilityCandidate{UNRESOLVED}` | compose::Capability / CapabilityCandidate |
| Ontology Resolver | 提取 Entity/Relation/Action/Event/State（如 Storyboard contains→ Scene；Video generated_from→ Storyboard）——**Skill Import 同时是 Ontology Discovery Pipeline** | RuntimeDefinition.relations（RelationDefinition） |
| Policy Analyzer | 从自然语言提取 必须审批/金额限制/权限/资源/时间（如"超过 $500 需要管理员批准"→ `{condition: refund.amount>500, requirement: approval.manager}`） | PolicyDefinition / PreconditionDefinition |

## 7. Artifact Ontology 与 Skill Graph

- **Artifact 一等对象**：Skill `produces` Artifact；Skill A → Artifact → Skill B 是组合基础（`{ id, name, type, produced_by, consumed_by }`）。
- **Skill Graph**：Registry 内部形成 DAG + Feedback Loop（如 YouTubeGrowth 的子技能树 + Optimization → ContentStrategy 反馈边）。MVP：邻接表结构 + 环检测（feedback loop 标记，不参与 DAG 排序）。

## 8. Import Pipeline（正式流程）

```
Upload → Raw Storage → Document Parser → AI Semantic Extraction → SkillIR
  → Capability Resolution → Ontology Resolution → Policy Analysis
  → Schema Validation → Semantic Validation → SkillAsset → Registry
```

**AI Parser 职责**：结构化提取 Goal/Sub-goals/Steps/Inputs/Outputs/Capabilities/Artifacts/
Preconditions/Postconditions/Policies/Metrics/Feedback loops/Constraints，并返回
`{ confidence, evidence, source_span }`。

**MVP 实现策略（无 LLM 环境）**：`AISemanticParser` trait（未来接 Pi/LLM，Phase 3 延期项）；
当前提供确定性 `MarkdownSkillParser`（标题层级/列表/表格 → 阶段/子技能/能力/工件/指标/反馈环，
置信度 1.0，span = 行号区间）——与 FakeEngine 同模式：接口就位，实现可替换。

## 9. Validation（四层，MVP 至少前两层 + 原则检查）

| 层 | 检查 |
|----|------|
| Schema Validation | JSON Schema 结构完整（IR 必填字段） |
| Semantic Validation | Input → Capability → Output 连通（该能力接受输入、产生输出） |
| Ontology Validation | Entity/Relation/Capability/Event 一致性（MVP 检查：Relation 两端实体已声明） |
| Policy Validation | 权限/审批/约束可编译为 PolicyDefinition |

## 10. Skill Registry（MVP 能力子集）

`Import / Browse / Search / Inspect / Version / Compare / Evaluate / Compose / Fork / Deprecate / Export`
（MVP：Import/Inspect/Version/Fork/Compose/Export/Deprecate；语义搜索 MVP 仅按 name/goal/capability 过滤，
Goal+Graph 语义搜索列 Non-goal）。

**生命周期**：`IMPORTED → PARSED → NORMALIZED → RESOLVED → VALIDATED → REGISTERED → ACTIVE`；
演化 `ACTIVE → FORKED → EXPERIMENTAL → EVALUATED → PROMOTED`；结束 `ACTIVE → DEPRECATED → ARCHIVED`。

**版本与 Lineage**：每个 Asset 记录 `{ source, origin, parent, derived_from, version, author, license, created_by, modified_by, evaluation }`——形成 **Lineage Graph**（youtube-growth@1.0 → fork → 1.1 → optimized-by Agent）。

## 11. Skill Composition 与 Mutation

- **Compose**：`Skill A + B + C → New Skill Candidate → 重新 Validate → Evaluate → Register`（新资产，原资产不动）。
- **Mutation（Fork 而非修改）**：`Skill v1 → Fork → Mutation → Candidate v1.1 → Evaluation → Promotion`——**Original 永远保留**（Phase 7 stale/version 机制同构）。

## 12. Skill Evaluation（MVP 静态指标 + 执行反馈占位）

指标：Success Rate / Execution Time / Cost / Failure Rate / Output Quality / User Acceptance
（+ Domain Metrics，如 YouTube：CTR/Retention/Watch Time/Subscriber Conversion/Revenue）。
MVP：Evaluation 记录指标定义与初始值；执行反馈接入 Phase 6/8 事件留接口（Execution → Metrics → Evaluation），完整闭环列 Non-goal。

## 13. Skill Export

Skill Asset → Export Compiler → **SKILL.md（MVP）**；未来 JSON/YAML/MCP/Agent-specific
（**Canonical Internal Representation，第三方格式只是 Export Adapter**）。

## 14. Implementation Spec（差距清单）

| # | 差距 | 内容 | 复用 |
|---|------|------|------|
| G1 | RawSkill 快照 | 导入 + content_hash + 只读 | — |
| G2 | SkillIR 结构 | 14 段语义 + provenance（confidence/evidence/source_span） | — |
| G3 | AISemanticParser trait + MarkdownSkillParser | 确定性解析（标题层级→阶段/子技能/能力/工件/指标/反馈环）；trait 留 LLM 接口 | FakeEngine 模式 |
| G4 | Capability Resolver | resolved / CapabilityCandidate(UNRESOLVED)——**不凭空创造** | compose::Capability |
| G5 | Ontology Resolver | Entity/Relation 提取 → RelationDefinition 候选 | runtime_def |
| G6 | Policy Analyzer | 自然语言 → PolicyDefinition 候选 | runtime_def |
| G7 | Validation | Schema + Semantic（+原则检查） | Phase 7 Validation 模式 |
| G8 | SkillAsset + Registry + 生命周期 | IMPORTED→…→ACTIVE；version/lineage；fork/deprecate | Phase 7 version/stale |
| G9 | Skill Graph | DAG + Feedback Loop（邻接 + 环标记） | Phase 5 subgraph |
| G10 | Compose | A+B→Candidate→revalidate→register | Phase 7 compose 语义 |
| G11 | Fork/Mutation | v1→fork→v1.1→evaluation→promotion；原资产保留 | Phase 7 version |
| G12 | Export | SKILL.md（canonical → 文本） | — |
| G13 | Capability Schema 化 | input/output_schema + postconditions 扩展 | compose::Capability |

## 15. MVP 验收（端到端 Case）

上传 `YouTube Channel Growth Skill`（Markdown），系统自动完成：
`Upload → AI Parse → 识别 Composite → 提取 7 阶段 → 提取 Sub Skills → 提取 Capability
→ 提取 Artifact → 提取 Metrics → 识别 Feedback Loop → 生成 SkillIR → 生成 SkillAsset → 注册 Registry`。

至少产生：`YouTubeChannelGrowth / NicheResearch / ChannelPositioning / AudienceResearch /
ContentStrategy / VideoIdeation / VideoProduction / AlgorithmGrowth / Monetization / AnalyticsOptimization`，
并支持 `[View Graph] [Compose] [Fork] [Export]`（MVP：API + 测试验收；UI 列 Non-goal）。

## 16. 测试规划

| 测试 | 验证 |
|------|------|
| M1 | RawSkill 导入 + hash + 只读（内容不可变） |
| M2 | Markdown → SkillIR：7 阶段/子技能/能力/工件/指标/反馈环 + source_span |
| M3 | Capability Resolution：resolved（registry 有）→ 绑定；unresolved → **Candidate 而非真实 Capability** |
| M4 | Ontology/Policy 提取 → RelationDefinition/PolicyDefinition 候选 |
| M5 | Validation：Schema/Semantic 失败路径（Input→Capability→Output 不连通 → 拒绝） |
| M6 | Registry：注册 + 生命周期推进（IMPORTED→…→ACTIVE；DEPRECATED） |
| M7 | Compose：A+B → Candidate → 重新验证 → 注册（原资产不动） |
| M8 | Fork：v1→v1.1 lineage；Promotion；原资产保留 |
| M9 | Export：Asset → SKILL.md（roundtrip 可再导入） |
| M-Rev | 原则反向：AI 推断无授权能力（不 bypass Policy）；未解析能力不进入任何执行路径 |

## 17. Non-goals（明确砍掉）

- ❌ 自动 Skill Marketplace / 自动跨平台发布 / 自动代码生成 / 自动修改生产 Skill
- ❌ 自动部署第三方 Capability / 完整 Skill Evolution Engine / 大规模多 Agent / 自动 Skill 交易
- ❌ 语义搜索（Goal+Graph 检索）、完整 Evaluation 闭环（执行反馈接入）、UI 页面（MVP 用 API+测试验收）
- ❌ LLM 真实解析（AISemanticParser trait 就位；确定性 Markdown parser 先行——无 LLM 环境）

## 18. 实现规划

| 步骤 | 内容 | 文件 |
|------|------|------|
| 1 | RawSkill + SkillIR + provenance 结构 | `src/skill/{mod,raw,ir}.rs` |
| 2 | AISemanticParser trait + MarkdownSkillParser（确定性） | `src/skill/parser.rs` |
| 3 | Capability/Ontology/Policy Resolver（Candidate 语义） | `src/skill/resolver.rs` |
| 4 | Validation（Schema/Semantic）+ SkillAsset + 生命周期 | `src/skill/asset.rs` |
| 5 | Registry（version/lineage/fork/deprecate）+ Skill Graph | `src/skill/registry.rs` `graph.rs` |
| 6 | Compose + Export（SKILL.md） | `src/skill/compose.rs` `export.rs` |
| 7 | Capability Schema 化（input/output_schema + postconditions） | `src/compose/capability.rs` |
| 8 | 测试 M1-M9 + M-Rev（YouTube 端到端） | `tests/skill_v2.rs` |
| 9 | 门禁（fmt/clippy/test 全绿）+ 提交 | — |

## 19. 与 Phase 8 的关系

```
Third-party World → Skill Import → AI Compiler → Skill Asset → Registry
  → Compose / Evolve → Skill → Phase 8 Runtime → Event → Experience → Future Learning
```

- **Phase 8 = 执行系统**（Skill → Runtime → Execute）
- **Phase 9 = 能力资产系统**（Third-party Skill → Compiler → Skill Asset → Runtime）

两者结合后，OpenAIP-OS 建立 **Capability Acquisition Layer**——为后续 Ontology 第三层、
Skill Genome、Skill Evolution、Self-Evolving Agent 提供可生长的"种子"。
