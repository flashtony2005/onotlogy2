# 技术债清零执行计划（8 项设计缺陷 · 测试先行）

| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-09-004 |
| 所属模块 | OpenAIP-OS-TechDebt |
| 版本 | V1.0 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-16 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发 |

## 1. 范围

清零 2026-08-16 设计评审结论中的 8 项缺陷：

| # | 缺陷 |
|---|------|
| 1 | V1/V2 双轨并行（planner/run.rs run_goal 与 cognitive loop 并存，无退役日期） |
| 2 | 三套演化引擎语义重叠（evolution.rs / evolution2/ / rule/evolution.rs） |
| 3 | 概念重复目录（kernel vs runtime_exec 同名文件、ontology/ 与 ir/odl/codegen、三套 registry） |
| 4 | 文档与代码漂移（README 停 V0.6、架构总图缺失、版本号三套） |
| 5 | 巨文件（oaip.rs 1183、mutation 1041、storage 796、planner/run 747、kernel 730） |
| 6 | panic 面大（全库 168 处 .unwrap()） |
| 7 | 分层靠约定不靠编译器（单 crate） |
| 8 | 流程文档空转（problem-summary / optimization-record 空表） |

## 2. 第一原理（全部决策依据）

1. **绿色是唯一前进方式**：每步改动后 `cargo test --all-targets` + `make check` 全绿；
   先测试、后代码；删除先于修补（AGENTS.md 分层构建 + 不保留向后兼容性）。
2. **行为契约测试优先**：任何「删/并/迁」之前，先把 canonical 路径的行为钉成契约测试。
   没有测试保护的删除不做。
3. **删除优先于修补**：将死代码不清理、不拆分、不搬进新 crate——先删。
4. **每项能力一个真相源**：Learn 一个口径、执行器一层、入口一条路径。
5. **编译期强制 > 口头约定**：模块 DAG 用 workspace crates 变成编译器检查。
6. **文档落后于代码，不领先**：文档重建放在代码收敛之后（M7），保证写下的就是现状。

## 3. 里程碑（依赖序）

执行顺序按依赖而非严重度：M1 删掉 V1 planner 后，#5 的 planner/run.rs 巨文件自动消失、
#2 的 evolution.rs 消费面砍掉一半；先删后拆、先删后清，避免在死代码上投入。

### M0 · 基线冻结（半天）✅ 已完成
- `cargo test --all-targets` 基线：**356 passed / 0 failed / 3 ignored**（2026-08-16 存档）。
- 本计划落盘（本文档）。
- 出口：基线绿 + 基线数字存档。

### M1 · 退役 V1 planner（#1，最大里程碑，4 步）
消费面已查清：`run_goal` 只被 CLI `agent run`、`server/run.rs`、4 个 v0_3_* 测试使用；
`action.rs` 只被 V1 planner + server 门面使用；`RuntimeKernel`（旧 runtime.rs）是 server 门面入口。

- **M1a 差集分析** ✅ 已完成 → [`m1-gap-analysis.md`](./m1-gap-analysis.md)：
  覆盖确认 C1–C6（Policy 前置条件 / 审批挂起恢复 / AutoApprove 闸门 / Memory / 多波重规划 /
  幂等均已由 V2 等价实现）；差集 G1–G10（认知 Trace 事件流、审批适配器、Pi 引擎迁移、
  审计落库、失败数据源、run 状态映射、health 数据源、via_workers 桥、测试迁移、UI 面板）；
  M1d 删除范围清单（planner/action/agent/runtime 门面 + wb_run/wb_memory/wb_approval 三表）。
- **M1b 契约测试（测试先行，不动产品代码）** 🔄 进行中 → [`m1-gap-analysis.md`](./m1-gap-analysis.md)：
  按差集把 v0_3_agent/policy/evolution 与 v0_9_ws_clamp 的行为改写为 V2 契约测试
  （`tests/v2_parity/`）；先跑红，逐个补齐 V2 实现到绿。
  - ✅ G1 认知 Trace 事件流：4 例契约测试 + `AgentLoop::with_trace` 实现（commit cecc44f），360 passed。
  - ⬜ G2 审批适配器 / G3 Pi 引擎 / G4 审计 / G5 失败数据源 / G6 状态映射 / G7 health /
    G8 via_workers / G9 v0_3 迁移 / G10 UI 面板。
- **M1c 切换入口**：CLI `agent run` 与 server `/run` 改走 cognitive loop；
  v0_3_* 测试退役（行为已被 v2_parity 覆盖）；全量确认 UI/WS 事件流不回归。
- **M1d 物理删除**：删 `planner/`、`action.rs`、`RuntimeKernel` 门面（server 直连 V2 Kernel）、
  相关 V1 表（wb_run / wb_memory / wb_approval 等，按差集清单）；codegen schema 同步减表。
- 出口：全绿 + 源码头寸收缩（planner 747 行消失）。

### M2 · 演化引擎归一（#2）
- canonical 定案：`evolution2`（Success/Failure 双通道 + Gate）= 经验层；
  `rule/evolution.rs` = 行为规则层；`skill` mutation = 资产层。三层边界写入文档。
- 测试先行：为三层各写契约测试（钉 Gate 5 闸门、rule Adoption 校验路径、skill mutation 不可原地改）。
- 删除 `evolution.rs` + `storage/evolution.rs` 三表（wb_pattern / wb_proposal / wb_learned）
  + server `/evolution` 端点；REQ-017/018 语义并入 evolution2 Gate。
- 出口：全绿 + 三套 Learn → 一套口径。

### M3 · 概念收敛（#3，先查证再动）
- 已查证：`runtime_exec` 不是重复实现，是 kernel 之上的世界执行层（D4 分离）——问题在命名撞车。
  方案：改名 `world_exec/` 或并入 kernel 为 exec 子层 + 模块文档写明「kernel 补偿引擎 vs 世界补偿执行器」层次。
- `ontology/`（V2 ABI：RuntimeDefinition → compose）并入 `compose/defs.rs`，
  「ontology」一词留给 V1 编译器（ir/odl/codegen）。
- worker/plugin/skill 三套 registry 属三个概念：先做重叠度评估，公共部分（manifest 解析 /
  health 探测）重复 >50% 则抽公共工具，否则保持三个（不强行统一）。
- 测试先行：每个移动前跑既有契约测试作 characterization，移动后全绿。
- 出口：src/ 内不再有同名异义文件。

### M4 · workspace crates 拆分（#7）🔄 进行中
- ✅ 第一步（27e402f）：`oaip-kernel` 独立 crate——L0 四 Primitive 零上层依赖
  （EventRef 下沉消除 kernel→evaluation 边）；`pub use oaip_kernel as kernel` 路径兼容。
- ✅ 第二步（5537d6b）：`oaip-abi` 独立 crate（odl/ir/codegen，语言前端）。
- ⬜ 剩余：oaip-storage / compose / cognitive / worker / server / cli（root 暂为门面）。

### M5 · 巨文件拆分（#5）✅ 主体完成
- oaip.rs 1200 行 → main 295 + 6 命令模块（f198cc7）；mutation 1041 → types/benchmark/validate
  （5c5aab7）。最大文件 401 行。

### M6 · unwrap 清零（#6）✅ 完成
- 预算门禁：scripts/unwrap-budget.sh 入 make check + CI（只降不升，168→129）。
- 核心路径 8 模块 `#![cfg_attr(not(test), deny(clippy::unwrap_used))]`；生产 unwrap 清理
  （server/v2 RwLock×6、odl section 栈×3）。剩余 129 处全在测试代码（7d4a540）。

### M7 · 文档重建（#4）✅ 完成
- architecture-v2.md 当前架构总图 + README 重写；6 篇旧架构文档标 [废弃]（1ce7840）。

### M8 · 流程文档激活（#8）✅ 完成
- problem-summary 迁入 8 条真实问题（含修复落点）；optimization-record 5 条带指标记录
  （e6ee4bb）。

## 4. 每个里程碑的统一节奏（测试先行铁律）

1. 写契约测试（红）
2. 补齐/对齐实现（绿）
3. 删除 / 迁移
4. `make check` 全绿
5. `graft build` 刷新 + 提交

## 5. 风险与对策

| 风险 | 对策 |
|------|------|
| M1d 删 V1 表/端点漏 UI 依赖 → 面板白屏 | M1a 差集清单覆盖 server/views.rs 全部读路径；M1c 后跑 serve + WS 冒烟 |
| M4 workspace 拆分路径地狱 | 放在删除完成后搬运量最小；以编译期依赖错误为向导 |
| 任一里程碑失败拖垮全局 | 每里程碑独立可交付，失败停在当前步，系统始终绿 |

## 修订日志

| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-16 | 初稿完成 |
