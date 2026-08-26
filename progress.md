# progress.md — 执行日志

## 会话 1(2026-08-18):阶段 0 基线审计 + 计划落盘

### 做了什么
1. 用户要求:从第一性原理出发,列出「实现生产可用可执行本体平台」的工作计划
2. 加载 planning-with-files-zh;graft map 摸底(2423 nodes);graft ask 定位(省 ~15.8k tokens)
3. 验证现状:make check 全绿(上一轮已收口 B1,20d947a 在 master)
4. 证据采集(见 findings.md):git tag 空、无 Docker 产物、无迁移框架、无性能基准、备份恢复仅文档态
5. 确立第一性原理 4 条 + DoD D1-D8 + 5 阶段计划;落盘 task_plan.md / findings.md / progress.md

### 测试结果
- 本会话未改代码,无新测试;基线 = make check 全绿(2026-08-18 实测)

### 关键决策
| 决策 | 理由 |
|------|------|
| 阶段0 证据驱动,不拍脑袋 | 每条差距带文件/命令证据 |
| 数据平面(阶段1)先于发布(阶段3) | 不丢数据是生产底线 |
| 5 阶段:0审计→1数据→2治理→3发布→4性能→5对外 | 依赖序:数据正确 > 安全约束 > 可部署 > 可测量 > 可宣称 |

## 会话 1 续1(2026-08-18):计划精修第一轮(批判性审视)

### 精修内容(第一轮批判性审视)
1. DoD 增 D9(资源与供应链边界)+ D10(备份 RPO/RTO 语义)
2. 阶段1 重排:S1c₀ 故障注入测试地基先行,迁移「不丢数据」靠它验证
3. 阶段2 增 S2e:cargo audit 进 CI
4. 新增「阶段收尾规则」:本阶段 DoD + 全量回归 + 前序 DoD 复验
5. 阶段0 标记 complete(差距清单已入 findings.md)
6. 决策落定:单机 Local-First、进程级 kill、S1c₀ 先行

### 用户已确认
- 生产形态:单机 Local-First ✅
- 故障注入:进程级 kill ✅
- 待定:性能基线是否 CI 门禁(阶段4 前)、备份 RPO 目标(阶段1 S1b 定)

## 会话 1 续2(2026-08-18):计划精修第二轮(深度层)

### 精修内容(6 个实质增量)
1. 拆分「schema 迁移」vs「本体演进」:S1a(DDL 迁移)/ S1f(本体行为兼容,旧数据在新 ObjectDef 下语义合法)——原 S1d 靶子打错(ObjectInstance 是 JSON 行)
2. S1c₀ 加事务原子性断言:中途 kill 无半写状态(多表写同事务)
3. S1b 先探 libsql backup API(VACUUM INTO / SQLite backup),能包装不造轮子(AGENTS.md 原则 5/6)
4. 审计不可篡改分层:S1h 存储层物理 append-only → S2d 逻辑层哈希链,跨阶段依赖显式化
5. 新增 S1g 时间语义:墙钟已确认 + 回拨护栏(证据:G11)
6. 阶段3 加 S3e 真实二进制 smoke test;S3d 演练须在真实数据上(environment-map 预发语义)
7. 新增风险登记表(6 项含缓解)

### 用户已确认
- 生产形态:单机 Local-First ✅ | 故障注入:进程级 kill ✅ | 两轮精修后待最终确认

## 会话 1 续3(2026-08-18):计划精修第三轮(用户点名 B 业务指标 + D 架构债务)

### 精修内容
1. **黄金场景(业务成功指标)**:Project→Task→AgentRun 域,真实二进制+真实数据+真实执行器;
   GS-1 定义/GS-2 执行/GS-3 治理/GS-4 演化/GS-5 恢复;每阶段收尾脚本化回归;DoD 增 D11
2. **架构债清零**:DoD 增 D12;阶段1 拆 1A(债清理)+ 1B(数据平面);债务证据化(D1 Effect 双实现、
   D2 Approval 双路径注释自认、D3 投影职责、D4 测试命名;crates/src 排除)
3. 阶段1 内部编号重排:S1a→S1b 迁移、S1b→S1c backup、S1f→S1g 行为兼容、S1g→S1f 时间语义、新增 S1i 黄金场景脚本
4. 阶段收尾规则加第 4 条:黄金场景脚本化回归

### 债务证据(已在 findings.md 归档)
- D1:world_exec/effect.rs 129 行(活跃)vs kernel/effect.rs 126 行(仅 Reversibility 被用)
- D2:approvals.rs:3 自认双路径;v0_3_approval.rs 注释同
- D3:src/projection 424 行 vs kernel projection 80 行
- D4:v0_x 测试钉活跃语义,命名误导

### 下一步
用户确认后:建立 goal,进入阶段 1(1A 债清理 A1-A4 先行 → 1B 数据平面)

## 会话 2(2026-08-20):Phase 2.5.7.2 Watcher 权限前置与错误状态生产化

### 已完成
1. 在 `PluginRuntime` 增加统一 `authorize_filesystem_read`：绝对路径、canonicalize、普通文件和 manifest `filesystem_read` 权限一次完成。
2. 在 `PluginCatalog` 增加宿主侧文件读取 preflight；AutomationEngine 不再复制彩票目录规则。
3. LotteryPlugin 改为复用同一套授权函数，移除重复的 source root 二次校验路径。
4. watcher 在读取 fingerprint 和 CSV 前先完成插件授权；授权、文件不存在、权限和格式错误均不更新 fingerprint、不入队。
5. watcher 扫描失败写入 `automation.source_scan_failed` 审计事件，并继续扫描其它业务包；修复数据源后下一轮可重试。

### 测试
- 新增未授权文件前置拒绝测试。
- 新增文件不存在 → CSV 格式错误 → 修复后成功入队的重试测试。
- `cargo test --lib lottery_plugin::tests`: 4 passed。
- `cargo test --lib automation::tests`: 6 passed。

## 会话 3(2026-08-20):Phase 2.6 自动运行生产化与业务结果展示

### 已完成
1. 新增 `LotteryResultPanel`：从成功的 analyze/backtest/review 任务提取结构化结果，展示统计、回测、Pi Agent 复核和数据溯源。
2. 接入 `AutomationPanel`，彩票结果展示不依赖宿主硬编码 pack 分支；无结果时不展示空面板。
3. 增加任务历史筛选：任务类型、状态、策略 ID、数据版本、时间范围。
4. 增加失败任务错误摘要聚合；保留完整错误和 Trace 追溯。
5. 增加每个任务的完整 JSON 折叠详情。

### 验证
- `npm test --prefix ui`: 23 passed
- `npm run build --prefix ui`: passed
- `cargo check --workspace`: passed
- `cargo test --lib automation -- --nocapture`: 8 passed
- `cargo test --lib lottery_plugin -- --nocapture`: 4 passed
- `git diff --check`: passed
- `/readyz`: `auditChain=valid`, `status=ready`
- 生产静态包已包含 Phase 2.6 面板和筛选/详情文案；已登录页面此前验证统计、回测、Pi Agent 三栏正常显示。

### 当前状态
Phase 2.6.1 与 Phase 2.6.2 完成；下一步为 Phase 2.6.3：成功率、失败率、重试次数、Pi Agent 复核和数据版本趋势展示。

## 会话 4(2026-08-20):Phase 2.6.3 自动运行质量趋势

### 已完成
1. 新增 `AutomationTrendPanel`，基于当前加载的最近 100 条以内任务在前端派生最近 14 个活跃日趋势，不新增图表依赖。
2. 展示成功率、失败率、累计重试次数和 Pi Agent 复核成功/失败；成功率分母只包含 succeeded + failed。
3. 增加数据版本时间线，同一 `input_version` / `fingerprint` 仅保留首次出现记录。
4. 增加分策略回测趋势；按 `strategy_family + strategy_id + strategy_version` 隔离，前区、后区分别与随机基线比较，不生成跨策略总分。
5. 增加免责声明，明确质量趋势和历史回测不构成中奖预测或投注建议。
6. 增加纯函数单元测试，覆盖质量聚合、版本去重和策略隔离。

### 验证
- `npm test --prefix ui`: 26 passed
- `npm run build --prefix ui`: passed
- `cargo check --workspace`: passed
- `cargo test --lib automation -- --nocapture`: 8 passed
- `cargo test --lib lottery_plugin -- --nocapture`: 4 passed
- `git diff --check`: passed
- 真实页面 `http://127.0.0.1:18421/` 已验证趋势面板显示 15 条任务、成功率 86.7%、失败率 13.3%、累计重试 4 次、Pi 复核 3/1、4 个数据版本和 statistics v1.0.0 的 3 次独立回测。

### 当前状态
Phase 2.6.3 完成；当前趋势为最近任务窗口的本地派生视图。长期趋势如需跨越 100 条任务，应在后续阶段增加后端分页或数据库聚合接口。

## 会话 5(2026-08-22):checkpoint 收口与就绪审计对齐

### 已完成
1. 修复此前 P0 CI 阻塞:清理 clippy 错误;unwrap 预算改为只统计生产代码并通过 `2 ≤ 10`。
2. 修复彩票插件 HTTP 集成测试:在 `tests/server_v2.rs` 注册 `DomainPackRuntime`,使 CSV 父目录经既有 fail-closed source-root 机制获得授权。
3. checkpoint `4569e94`(`checkpoint: green backend and frontend gates`)创建于 `codex/checkpoint-ci-green`,当时工作区干净。
4. 后端门禁 `make check` 通过;前端 `npm run typecheck && npm run test` 通过(5 files / 32 tests)。
5. 按 2026-08-22 代码证据对齐 `findings.md` 与 `task_plan.md`:G1-G14 与架构债 D1-D4 区分历史基线和当前结论。

### 当前结论
- Phase 0-2 完成;Phase 3 mostly complete。
- 发布工程剩余项:首个 release tag、生产规模真实数据备份/恢复演练、实测 RPO/RTO 记录。
- 下一优先级:先收口发布工程,再进入 Phase 4 性能/负载基线与 SLA。
- 工程质量后续项:拆分约 1859 行的 `src/automation.rs`,避免业务包运行时继续膨胀。

### 下一步
1. 准备首个 release:确定版本号、release notes、构建产物清单和 tag 流程。
2. 设计生产规模备份/恢复演练脚本,记录备份窗口、恢复耗时和数据一致性校验。
3. 演练通过后进入性能基准与 SLA 文档。

## 执行日志(2026-08-22 · D7 第一轮)

- 新增 `benches/performance.rs`,用 Criterion 固定四条核心路径负载:ODL 编译、投影重放、世界查询、审计写。
- 新增 `make bench`;Cargo 主包版本从 0.9.0 提升到 0.14.0 候选。
- 首轮 Apple M4 本机基线已入档 `docs/03_架构设计/performance-baseline.md`:ODL 编译约 48.3µs,
  1,000 事件投影约 335.5µs,500 对象查询约 248.0µs,审计链追加约 28.0µs。
- 边界:这是核心路径微基准,不替代 S4b 的真实 HTTP/WS 并发压测和生产 P99 证据。

## 执行日志(2026-08-22 · D7 第二轮)

- 新增真实二进制负载脚本 `scripts/performance-load.py` 和 `make load-baseline`,固定
  10 次 submit/approve、并发 5、3 个 WebSocket 客户端。
- 首轮负载暴露共享 libSQL 连接并发开启事务的 Critical bug;已为所有进程内写事务加
  共享写锁,并新增 24 路并发 world command 回归测试。
- 修复后负载 10/10 成功、0 失败;wall time 0.035347s,吞吐 282.911 rps,P50/P95/P99
  为 16.651/17.986/17.986ms,6 类 fabric 事件各 30 次。
- `make check`、`make golden`(GS-1~GS-5)、`make ops-smoke` 全部通过。
- S4b/S4c 收口:负载基线、控制面 token bucket 边界、WS best-effort 与 `after_id`
  最终一致语义已入档 `docs/03_架构设计/performance-baseline.md`。

## 会话 6(2026-08-26):活跃目标优化的数据库唯一边界

### 已完成
1. 将同一项目只允许一个 `queued/running` 目标优化任务的约束从进程内 mutex 升级为 SQLite/libSQL 部分唯一索引。
2. Schema 版本从 19 提升到 20，提供 V19→V20 迁移和回滚；canonical schema 保持一致。
3. 入队路径改为普通 INSERT，识别活跃项目唯一冲突后返回 `WB2112`，Business Loop 映射为明确的用户错误。
4. 移除优化入队的进程内闸门与全量列表扫描；普通 dedupe-key 冲突仍保持既有幂等语义。
5. 增加存储层多实例共享连接回归和 HTTP 集成测试，覆盖第二个写入者拒绝与取消后重新入队。

### 验证
- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --all-targets -q`
- `cd ui && npm run typecheck && npm test`(40 passed)
- 真实二进制黄金场景 GS-1~GS-5
- `make ci`

## 执行日志(2026-08-26 · D10 备份演练工程)

### 已完成
1. 新增真实二进制备份/恢复演练脚本 `scripts/backup-rpo-rto-rehearsal.py`：
   启动服务 → 公开 API 写入确定性数据 → 停服 → backup → restore → 重启恢复服务 →
   readyz/auditChain/全量计数/marker/抽样 ID/payload 校验 → 输出 JSON 证据。
2. 新增 `make backup-rehearsal`，默认 64 MiB 冒烟规模，参数可覆盖。
3. 修复演练中发现的验证边界：CLI `object list` 固定 1000 行，只适合运营视图；
   规模化恢复校验改为对恢复库只读 SQL 计数与抽样。

### 验证
- 1 MiB 真实二进制闭环：PASS。
- 8 MiB Makefile 入口闭环：PASS。
- 64 MiB 真实二进制冒烟：PASS；2,048 对象，源库+sidecar 67.203125 MiB，
  backup 0.249s，restore CLI 0.214s，恢复后 readyz 0.290s，总 RTO 0.505s；
  全量计数、marker、抽样 ID 与 payload 长度一致，auditChain valid。
- 明确边界：本次是静默快照完整性/RTO 冒烟；生产 RPO 仍需外部 15 分钟调度与真实写入窗口证据，
  正式发布还需 1 GiB 演练。

## 执行日志(2026-08-26 · Reasoner 规模化基准)

### 已完成
1. 新增受控规模探针 `benches/reasoner-scale.rs`，测量完整管线：
   事实生成 → `runtime_world_to_knowledge()` → `Reasoner::new() + is_consistent()`
2. Apple M4 release 模式实测数据：

| 规模 | KB 公理 | 转换耗时 | 推理一致性耗时 |
|------|---------|----------|---------------|
| 100 | 203 | 0.3ms | 3.0ms |
| 500 | 1,003 | 0.4ms | 34.5ms |
| 1k | 2,003 | 0.4ms | 118.5ms |
| 5k | 10,003 | 1.6ms | 10,901ms (~11s) |
| 10k | 20,003 | 8.4ms | 84,230ms (~84s) |

3. 增长曲线确认二次方：5k→10k（2×）→推理时间 7.7×

### 根因定位
- 瓶颈完全在 `Reasoner::new() + is_consistent()`，转换是微秒级
- `runtime_world_to_knowledge()` 将所有对象 ID 放入单个 `DifferentIndividuals`
- 上游 owl-dl-reasoner 将 N 元不同性展开为 N²/2 对 pair（lib.rs:L2124-L2131）
- Business Loop discover/decide/execute 均走 whole-world 一致性 gate

### 结论
- 本体论建模和演化功能正确；分析在小规模（≤500 对象）可用
- 企业级规模（≥1k）时一致性检查成为吞吐瓶颈

## 执行日志(2026-08-26 · Reasoner 身份投影修复)

### 已完成
1. 用二进制身份分区替换全局 N 元 `DifferentIndividuals`：每个对象获得
   `ceil(log2(N))` 位编码，每个位的 0/1 partition class 互斥。任意两个不同 storage ID
   至少在一个位上冲突，因此完整 pairwise distinctness 语义保留。
2. 该投影不选择“身份敏感个体”，递归/nested nominal、functional、inverse-functional 和
   cardinality witness 都继续进入同一成熟推理语义，不引入静默正确性缺口。
3. 新增回归覆盖：函数型关系两个 witness、exact/max cardinality 两个 witness、嵌套 nominal
   目标类，以及 asymmetric relation 双向 world edge 的 fail-closed gate。
4. 新增 `make reasoner-scale-sla`，固定 `5,000 objects <= 1,000 ms` release 门禁，
   并接入本地 `make ci` 与 GitHub Actions。
5. 性能基线和边界已入档 `docs/03_架构设计/performance-baseline.md`。
6. 黄金场景核心回归改为默认确定性 `/bin/sh` process executor；显式设置
   `OAIP_DSH_COMMAND` 时仍可验证特定外部 agent。这样 GS-1~GS-5 不再被本机
   dsh 凭证配置影响，同时保留真实进程执行、世界变更和恢复断言。
7. 消除 data-consistency 缓存测试的并行 flake：校验核心改为可注入本地 LRU，
   生产入口继续使用进程级缓存；测试不再把共享缓存的未淘汰状态当作断言。

### Apple M4 release 实测

| 对象数 | KB 公理 | 转换耗时 | 推理一致性耗时 |
|-------:|-------:|-------:|---------------:|
| 100 | 909 | 0.8ms | 6.1ms |
| 500 | 5,511 | 1.2ms | 23.4ms |
| 1k | 12,012 | 1.4ms | 33.4ms |
| 5k | 75,015 | 6.1ms | 211.1ms |

修复前 5k 一致性约 80.65s；修复后全管线约 217.7ms，SLA 门禁通过。

### 验证

- `make ci`: PASS（含 fmt/clippy/workspace tests/unwrap budget/UI audit/Reasoner SLA）。
- `make golden`: PASS，GS-1~GS-5 全链路通过；5k release SLA 实测 217.9ms。
- `cd ui && npm run typecheck && npm test -- --run`: PASS，40 tests。
- `make backup-rehearsal`: PASS；2,048 对象、67.203MiB 源库，
  backup 0.257s，restore CLI 0.184s，恢复后 ready 0.247s，总 RTO 0.432s。

### Flake 修复后复验（2026-08-26）

- `data::tests` 并行复跑 20 次：PASS。
- `make ci`: PASS；release 5k 全管线 236.3ms（<=1000ms SLA）。
- `make golden`: PASS，GS-1~GS-5 全链路通过。
- `cd ui && npm run typecheck && npm test -- --run`: PASS，40 tests。
- `make backup-rehearsal BACKUP_TARGET_MIB=67`: PASS；2,048 对象、67.203MiB，
  总 RTO 0.407s。

## 执行日志(2026-08-26 · S3d 发布收尾)

### 已完成
1. 修正升级/回滚演练对全局 Cargo target 目录的隐式假设：`OAIP_BASE_REF` 构建旧版时
   强制使用隔离 target；真实二进制冒烟在需要构建时显式写入仓库 `target/`。
2. 升级/回滚演练显式读取 `wb_meta.schema_version`，区分两类合法发布路径：
   schema 变化时旧 binary 必须拒绝新库并返回 `WB2101`；同 schema 补丁发布允许停服后
   直接回滚旧 binary。快照回滚断言在两条路径中均保留。
3. 用真实 release binary 重演 schema 19→20：基线 `0a14d98`、候选 `cff1cd9`，
   迁移、backup/restore、降级保护、快照回滚全部 PASS。
4. 用真实 release binary 重演 `v0.14.0`（schema 20）→ reasoner 修复候选（schema 20）：
   同 schema 直读、原对象校验、备份/恢复和升级前快照回滚 PASS。
5. 真实二进制运维冒烟 PASS：`/healthz`、`/readyz`、`/metrics`、SIGTERM、备份、恢复。
6. 完成 1 GiB 规模档备份/RTO 演练：31,226 个对象、源库+sidecar 1020.292969 MiB，
   backup 9.636102s，restore CLI 5.996403s，恢复后 readyz 9.704044s，
   总 RTO 15.701049s；全量计数/marker/抽样/payload 一致且审计链 valid。
7. 将跨 schema、同 schema 和 1 GiB 规模档证据写入部署运维文档。
8. 修复 `make ci` 的前端假绿：`ui` 目标与同名目录冲突导致 Make 认为目标最新；
   声明 `.PHONY: ui` 后确认 `make ui` 实际执行 npm install/build。

### 验证
- `OAIP_BASE_BIN=<schema19> make ops-rehearsal`: PASS（19→20）。
- `OAIP_BASE_BIN=<v0.14.0> make ops-rehearsal`: PASS（20→20）。
- `make ops-smoke`: PASS。
- `make backup-rehearsal BACKUP_TARGET_MIB=1016`: PASS。
- `make ci`: PASS；5k release SLA 225.1ms <= 1000ms。
- `cd ui && npm run typecheck && npm test -- --run`: PASS，40 tests。
- 修复后 `make ui`: PASS，production build 1827 modules。

### 边界
- 本次 1 GiB 规模档仍是停止服务后的静默快照完整性/RTO 演练。
- 生产 RPO ≤15 分钟还需要外部调度器成功执行记录、失败告警证据，以及一次真实写入窗口
  内的恢复验证。

## 执行日志(2026-08-26 · S5a 从零部署演练收口)

### 已完成
1. 消除部署文档与产物的镜像版本漂移：Compose 固定标签更新为 `openaip-os:0.14.1`；
   container smoke 和 release manifest 默认镜像改从根 `Cargo.toml` 动态取工作区版本。
2. Compose 冒烟从“容器健康”升级为真实持久化演练：登录后创建 `persist-test` 项目、写入
   Customer 事件，先断言 `module:root --contains--> cust-persist-001`，再执行
   `compose stop --timeout 15 → compose up --detach`，重启后重新登录并按同一 world query 读回。
3. 增加 Compose 镜像标签和工作区应用版本的一致性守卫，并要求 `jq`；API 响应非零码时输出
   response body，避免 HTTP 4xx 被 curl 静默吞掉后无法定位。
4. 修复计划外发现的 API 契约缺陷：浏览器 UI 发送 camelCase 的 `objectType`，但
   `ProjectEventReq` 只接受裸 snake_case 字段，导致项目面板“添加对象”实际返回 422。
   服务端 DTO 现声明 `#[serde(rename_all = "camelCase")]`，并用 `tc_sf7` 钉住 create → event →
   world query 全链路。

### 验证
- `bash -n scripts/compose-smoke.sh scripts/container-smoke.sh scripts/release-manifest.sh`: PASS。
- `make compose-smoke`: PASS；真实镜像构建约 2 分钟，覆盖 healthcheck、世界写入、
  `stop → start` 后 named volume 数据读回和资源清理。
- `cargo test tc_sf7_project_event_accepts_camel_case_contract`: PASS。
- `make release-manifest`: PASS，`container.image=openaip-os:0.14.1` 与工作区版本一致。
- S5a 提交门禁：pre-commit fmt/clippy/workspace tests PASS；提交为 `04b6597`。

### 边界
- 本次 S5a 以仓库提供的生产 Compose 路径为准验证“文档指引 → 生产实例”；尚未在另一台
  物理空机器上重复人工执行。
- 外部 15 分钟备份调度、失败告警和真实写入窗口 RPO 证据仍归入运营验收。
