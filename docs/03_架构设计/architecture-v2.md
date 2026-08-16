# OpenAIP-OS 当前架构总图（V2 · 2026-08-16）

> 本文是唯一「当前架构」文档（M7 重建）。V0.x 时代的架构文档已标 [废弃] 归档于
> `00_归档_legacy` 与 `03_架构设计`（architecture-total.md 等）。代码先行，文档跟随。

## 1. 一句话

可执行本体平台内核：`.odl` 定义世界，**一条认知闭环**（Observe → Decide → Act → Verify）
驱动 Agent 与世界交互；Kernel 四 Primitive 是唯一状态源，EventLog 可回放（I1）。

## 2. 分层

| 层 | 模块（src/） | 职责 | 关键不变量 |
|----|--------------|------|-----------|
| L0 Kernel | `kernel/` | Context / Fiber / Effect / Relation 四 Primitive + 状态机 + 补偿 + 事件轨 | P1 补偿、P2 生命周期隔离、P3 时间极简、P4 认知分离 |
| L1 语言 | `ir/` `odl/` `codegen/` | `.odl` → IR → Rust/SQL/JSON | 确定性编译，无 LLM |
| L1 定义 | `compose/defs.rs` | RuntimeDefinition（Compiler → Runtime ABI）| ODL/JSON/LLM 同构 |
| L2 组合 | `compose/` | Service / Capability / Policy / Task | Policy 是约束非 Primitive |
| L2 世界执行 | `world_exec/` | Effect 执行 / 补偿执行（D4 双执行器） | State = Relation 投影 |
| L3 认知 | `cognitive/` | Agent / AgentLoop / Memory / PiEngine / runner / approval | 认知 ≠ 事实（P4） |
| L3 演化 | `evolution2/` `rule/` `skill/` | 经验分账 → Gate → 行为规则 → Skill 资产 | Learn ≠ Permission；一层口径 |
| L4 Worker | `worker/` | Skill → Capability → Router → Worker（CLI/HTTP/MCP） | Provider Agnostic；fail-closed |
| L4 插件 | `plugin/` | 外部能力接入 | Secret ≠ EventLog |
| L4 通信 | `fabric/` | Agent Communication Fabric（OAB）：AgentMessage / Session / Adapter / Router（Intent→Capability→Policy→Selection）| Adapter 管协议、Bridge 管语义；不直传 Agent 消息；通信事件全量入 Event Log（契约 agent-fabric-v1.md） |
| L5 观察 | `trace/` `server/` `ui/` | TraceEvent 单词汇表 → REST/WS/UI | 三层视图钳制同源 |
| L5 存储 | `storage/` | 对象/关系/事件/run/评估/用户 | SQL 参数化；迁移版本化 |
| L5 认知状态 | `cognitive/persist.rs` | CognitiveStore（记忆/演化快照/SessionLog） | 显式 save/restore |

## 3. 执行路径（唯一入口）

```
CLI `oaip agent run` / server `POST /run`
  → cognitive::runner::run_agent        （入口编排：审批 resolver 循环）
    → AgentLoop::run_until_done          （Observe→Decide→Act→Verify）
      → CognitiveEngine::decide          （PiEngine / FakeEngine / NoOpEngine）
      → TaskExecutor::step               （Policy → 审批 → Effect → 投影）
  → 终局：trace sink → wb_trace_event；run 状态 → wb_run；
    Kernel 审计事件桥（approval.* / gate_approved）→ /audit 可查（I1）
```

- 审批：Waiting fiber 是唯一待审队列；CLI = DecisionCache + stdin；server = pending
  注册表 + mpsc + 超时看门狗（Expire 审计与 Reject 区分）。
- 演化：`EvolutionGate`（7 条件 AND）替代审批仅限「Validated + 无失败 + 可补偿」，
  Gate 失败 = WAIT 非 DENY（I5）。
- 模拟：S1 Replay / S2 Projection / S3 Counterfactual 全部是假设轨（I2），永不写 EventLog。

## 4. 数据与事实

| 事实 | 位置 | 说明 |
|------|------|------|
| 世界对象/关系/事件 | `wb_ontology_object` / `wb_relation_edge` / `wb_trace_event` | storage 被动层；事件由调用方显式 append |
| run 元数据 | `wb_run` | V2 run store（G7 重定位保留） |
| 评估（派生） | `wb_evaluation` | Evaluation = Pure Function(EventLog)，可删除重建 |
| 认知状态 | `wbc_cognitive`（CognitiveStore） | 记忆/演化快照/SessionLog/插件状态 |
| Kernel 事实 | Kernel 事件轨（进程内） | 审计经 bridge 进 wb_trace_event |

## 5. 关键历史决策（M0–M8 技术债清零摘要）

1. **M1**：V1 planner/action/agent/runtime 门面删除——双轨终结，AgentLoop 唯一路径。
2. **M2**：V1 evolution 三表删除 + 审计查询入口（/audit 读事实轨）。
3. **M3**：runtime_exec → world_exec；ontology/ 并入 compose/defs.rs。
4. **M5**：oaip.rs → 6 命令模块；mutation → types/benchmark/validate。
5. **M6**：unwrap 预算门禁（只降不升）+ 核心路径 deny(clippy::unwrap_used)。
6. **M7（本文）**：文档与代码对齐，旧文档 [废弃] 归档。

## 6. Workspace 依赖 DAG（M4 完成）

```
oaip-abi（语言前端：odl/ir/codegen）
  → oaip-kernel（L0 四 Primitive，零上层依赖）
      → oaip-session（Session Log + run 生命周期）
          → oaip-trace（TraceEvent 单词汇表 + 三层视图）
── 应用层（根 crate 门面内）──
  compose / cognitive / world_exec / evolution2 / rule / skill
  worker / plugin / simulation / health / diagnosis
  server / auth / cli（bin）
```

`cargo build -p oaip-kernel` 不依赖任何上层——「插件不得污染 Kernel」编译期强制。
八个基础设施 crate 全部独立；应用层留在根门面（迁移价值低、环已解）。

## 7. 已知边界（诚实声明）

- ~~storage 世界对象尚未投影进 Kernel 关系~~（P5.4 已落地）：CLI/server 的 V2 闭环
  在 run 起点经 `Projection::project_from_storage` 全量重建 Kernel 世界——Agent 首次
  观察即见 Storage 世界；per-run 全量策略，跨 run 常驻 Kernel + 增量水位属后续里程碑
  （P3，契约 docs/03_架构设计/projection-v1.md）。
- ~~审计查询基于 run 级桥接~~（P5.6 已落地）：审计事件（approval.* /
  evolution.gate_approved）产生时即落库 wb_trace_event（Kernel audit sink 同步
  append），与 Run 生命周期解耦——进程崩溃时已产生的审计不丢失；
  /audit 直读事件表。
