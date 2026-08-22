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
