# 项目总览
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-01-001 |
| 所属模块 | WorkBuddy-Overview |
| 版本 | V1.5 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
说明 OpenAIP-OS 的产品定位、MVP 战略取舍、核心哲学与关键干系人，统一团队对"第一阶段只做什么"的认知，避免第一版做成"完整 Palantir"。

## 2. 业务/技术背景
- 参考文档：`00_归档_legacy/OpenAIP-OS-V1.1.md`（Whitepaper：Ontology-Centric + Local First + Agent Native 的开源企业智能操作系统）、`00_归档_legacy/OpenAIP-OS-V1.3.md`（Compiler Report：MVP 路线 .odl → IR → Rust + Turso → ObjectEngine）、`00_归档_legacy/Palantir-OSS-Reference.md`（核心借鉴：Ontology 即类型化 API、IR、多语言生成、codegen/runtime 分离）。
- 受 `project-constraint.md` 与 `glossary.md` 约束。

## 3. 核心规则 & 约束（重点）
- [强制] **MVP = V0.3**：V0.1 交付 Ontology Runtime Kernel、V0.2 补 World Model、V0.3 接入 Agent（Pi 作 Cognitive Engine + 自有 Planner + 观察 UI）；不实现完整 AI OS（不含完整 GraphRAG / Policy / Workflow / Evolution 全套）。
- [强制] 可观察性与内核同生：V0.1 落地 Event Bus + TraceEvent(world) + CLI 观察，V0.3 升级为三层图形 UI + CognitiveEvent（Human 在环）。
- [强制] 核心模型仅 **Object / Relation / Event** 三要素：Object（V0.1）、Relation 与领域 Event（V0.2）成型，MVP（V0.3）齐备。
- [建议] 一切高级能力（GraphRAG / Policy / Workflow / Evolution）以插件形式在 Kernel 之上后生长，Kernel 保持长期稳定；Agent 接入（Pi 作 Cognitive Engine，经 `AgentEngine` trait）在 V0.3 以插件形态落地，未来可替换引擎。
- [强制] 立项与排期必须对照 `09_迭代与版本/sprint-plan.md` 的 V0.1~V0.5 演进路线，禁止在 MVP（V0.3）提前引入 V0.4+ 能力。

## 4. 详细设计/内容

### 4.1 项目定位
OpenAIP-OS 长期定位是 **Ontology-Centric + Local First + Agent Native** 的开源企业智能操作系统。但这是**最终形态**，不是起点。

最终形态的构成（供参考，非 MVP）：

```
Ontology + Knowledge Graph + GraphRAG + Agent Runtime + Policy Engine
                              ↓
              Enterprise AI Operating System
```

内核先做最小可运行闭环（V0.1）：

```
 .odl → Ontology Compiler → Ontology IR → Runtime Kernel → Turso → Object CRUD
```

一句话：**让一个对象拥有生命。** 随后 V0.2 让世界成型（+Relation+Event），V0.3=MVP 让 Agent 住进世界并被人理解。

### 4.2 最大风险判断
本项目最大风险不是技术难度，而是**第一版做得太像"完整 Palantir"**。

正确路线类似 Unix：先做一个极小 Kernel，让世界模型可以运行，再通过分形扩展生长成完整系统。MVP 应追求 **Ontology Kernel 100%**，而非 Palantir 10%。

### 4.3 核心哲学：Everything is Object
不要从 `Database → Table → CRUD` 开始，而应从 `Object` 开始：

```
Object
 ├── Data
 ├── Relation
 ├── Event
 └── Metadata
```

V1.1 已定义核心模型为 `Object / Relation / Action / Policy / Event`，但 MVP 只保留：

| 要素 | 含义 | 后生长关系 |
|------|------|------------|
| Object | 世界中的实体 | 基础（V0.1） |
| Relation | 世界连接 | 基础（V0.2） |
| Event | 时间维度（发生过什么） | 生命周期 V0.1 / 领域事件 V0.2 |
| Action | 对象的未来行为 | 最小执行 V0.3 / 完整引擎 V0.4 |
| Policy | 行为约束 | = Action 的约束，后生长 |
| Memory | 经验积累 | = Event 的积累，后生长 |
| Agent | 智能操作者 | V0.3 接入 Pi 作 Cognitive Engine（=MVP），V0.5 扩展自治 |

### 4.4 最终判断
本项目的真正创新不是"做一个开源 Palantir"，也不是"一个给 Agent 用的数据库"，更不是"Pi 增强版"，而是**第一个 Agent Native 的可观察世界模型操作系统**：

> **Agent 可以行动 + 人可以理解 + 人可以干预 + 系统可以学习**

这是区别于 LangChain / AutoGen / CrewAI 等普通 Agent Framework 的核心竞争力——当前 Ontology + Event + Runtime 天然适合承载"可观察"这一层。

**核心资产切分（关键）**：OpenAIP-OS 真正难复制的部分是 **Ontology Kernel + World Model + Agent Runtime（Planner / Policy）+ Decision Trace**；Pi 只是可替换的 Cognitive Engine（经 `AgentEngine` trait 接入，未来可换成 Claude / Qwen / LocalLLM），不应反过来让 Pi 成为系统本体。详见 `03_架构设计/pi-agent-integration.md` 与 `03_架构设计/architecture-total.md` 4.7。

分形生长的三层最小证明：
1. **V0.1**：一个 `.odl` 文件，可以生成一个可运行的世界对象（内核成立）；
2. **V0.2**：Object + Relation + Event 聚合成可查询的世界模型（世界成型）；
3. **V0.3 = MVP**：Agent（Pi 作 Cognitive Engine）基于世界模型决策，且人能**看到证据、理解推理、并介入审批**（Observe → Understand → Intervene，产品价值成立）。

若 V0.3 闭环成立，Action 编排 / Policy / GraphRAG / Evolution 都会自然生长——这也契合"从极简内核，通过分形循环生长成完整 Ontology OS"的路线。

### 4.5 业务价值
| 价值维度 | 说明 | 量化指标（待补） |
|----------|------|------------------|
| 确定性 | 世界模型以类型化 Object 表达，避免散落表结构 | 对象Schema 收敛率 |
| 可演进 | 内核稳定，能力插件化，避免推倒重来 | 插件接入耗时 |
| 本地优先 | Local First，数据主权留在客户侧 | - |

### 4.6 干系人
| 角色 | 姓名/团队 | 关注点 |
|------|-----------|--------|
| 产品负责人 | lushaohui | MVP 范围取舍、内核极简 |
| 核心研发 | 待定 | Compiler / Runtime / Storage |
| 早期用户 | 待定 | 能否用 .odl 跑通对象闭环 |

## 5. 风险点 & 注意事项
- 范围蔓延：容易把 V0.3 的 Agent/策略"顺手"提前进 V0.1/V0.2，需以本文 4.2 与 sprint-plan 版本表为回拉锚点。
- 干系人对"不做 AI OS"的认知需对齐，避免期望错位。
- 内核一旦为某业务类型（如 Customer）写特化逻辑，会破坏极简原则（见 `03_架构设计/architecture-total.md` 原则 1）。

## 6. 关联文档
- [../01_项目总览/project-constraint.md](../01_项目总览/project-constraint.md)
- [../01_项目总览/glossary.md](../01_项目总览/glossary.md)
- [../03_架构设计/architecture-total.md](../03_架构设计/architecture-total.md)
- [../03_架构设计/module-split.md](../03_架构设计/module-split.md)
- [../03_架构设计/agent-observation-design.md](../03_架构设计/agent-observation-design.md)
- [../03_架构设计/pi-agent-integration.md](../03_架构设计/pi-agent-integration.md)
- [../09_迭代与版本/sprint-plan.md](../09_迭代与版本/sprint-plan.md)
- [../09_迭代与版本/todo-backlog.md](../09_迭代与版本/todo-backlog.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成 |
| V1.1 | lushaohui | 2026-08-11 | 填入 MVP 可行性分析：定位、最大风险判断、Everything is Object 哲学、最终判断 |
| V1.2 | lushaohui | 2026-08-11 | MVP 纳入 Agent Observation Kernel；最终定位升级为 Agent Native 可观察世界模型操作系统 |
| V1.3 | lushaohui | 2026-08-11 | 纠正：MVP 包含 Agent 接入（Pi 作 Cognitive Engine）；更新哲学表与最终判断中的 Agent 定位 |
| V1.4 | lushaohui | 2026-08-11 | Pi 降级为 Cognitive Engine；补充核心资产切分（Ontology+WorldModel+AgentRuntime+Trace），非"Pi 增强版" |
| V1.5 | lushaohui | 2026-08-11 | 方案B对齐：明确 MVP=V0.3；哲学表/最终判断按 V0.1 内核→V0.2 世界→V0.3 Agent 闭环重述；Event 分层 |
