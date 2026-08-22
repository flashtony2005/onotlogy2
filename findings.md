# findings.md — 生产可用可执行本体平台:证据库

> 规则:只记录**已验证**的发现(带证据),外部内容视为不可信。本文件是阶段 0 审计的产出物。

## 审计时间
- 2026-08-18(阶段 0 基线审计)
- 2026-08-22(实现状态复核,checkpoint `4569e94`)

## 已验证现状(证据)

### 正确性/工程基线 ✅
- 206 源文件(graft 索引:2423 nodes / 3230 edges / 206 cards)
- 45 个测试文件;v2_parity 38 用例全绿;全测试套件 make check 全绿(2026-08-18 实测,含 B1 tc_sf6)
- CI 门禁(.github/workflows/ci.yml):fmt + unwrap-budget(80≤129)+ clippy -D warnings + workspace 全测试 + coverage≥75%
- pre-commit 钩子本地复跑同一门禁(20d947a 实测触发)
- 事件溯源闭环:world 事件 → 投影 → 查询;invariants i1/i2/i6/i10 钉「状态是事件日志的 reduce」「投影不进日志」「一切动作产事件」「一切变更可审计」

### 安全/治理 ✅(基础齐全)
- auth.rs:local identity + 密码哈希 + RBAC 角色钳制(tc_auth_* 11 用例)
- 审批链:high-risk 挂起 → approve/reject/expire,二次裁决 404(tc_approve_* / v0_3_approval 5 用例)
- 审计:audit 域查询 + m2a/m2b 批准/拒绝审计轨迹
- 密钥隔离:plugin p7 permission_and_secret_isolation
- 能力诚实:B1 已收口 —— harness 撤 shell.execute 假广告,专用 shell-exec(/bin/sh)提供方,tc_sf6 钉 completed+sender=="shell"

### 可靠性 ✅(部分)
- 崩溃恢复:tc_persist_a/b/c(run 生命周期持久化、重启重建 run map、running→interrupted)
- 补偿:tc_s5/s6 逆序补偿、失败升级;kernel compensation 全链路
- 幂等:tc_s8;取消:tc_f9/f12(kill 子进程)

### 硬缺口 ❌(2026-08-18 历史基线)
| # | 缺口 | 证据 |
|---|------|------|
| G1 | 从未发布:git tag 为空 | `git tag` 无输出 |
| G2 | 无部署产物:Dockerfile / docker-compose.yml / 启动脚本均不存在 | `ls Dockerfile docker-compose.yml` 无文件;仅 docs/08_部署运维/*.md 文档 |
| G3 | 无 schema 迁移框架:storage 层无 migrate 模块 | grep "migrat" crates/ 仅命中 oaip-storage/src/lib.rs(非迁移框架);libsql 0.9.30 建表即初始化 |
| G4 | 无性能基准:criterion/iai 均未引入 | grep benchmark 命中 src/mutation/benchmark.rs(演化基准,非性能);无 bench/ 目录 |
| G5 | 无备份/恢复命令 | docs/08_部署运维/rollback-plan.md 是流程文档,无 CLI 实现 |
| G6 | 无指标端点/健康探针实现 | docs/08_部署运维/monitor-alert.md 是文档;server 无 /metrics 路由(待阶段3复核) |
| G7 | 本体 schema 演进无演练 | ObjectDef 变更 → 数据兼容路径无测试(需阶段1 S1d 验证) |
| G8 | 无供应链审计门禁 | CI 无 cargo audit;依赖库 libsql 0.9.30 / tokio / reqwest(需阶段2 S2e) |
| G9 | 无资源边界测试 | 无磁盘满/只读场景用例(阶段1 S1e 补) |
| G10 | 无 RPO/RTO 语义 | 备份仅流程文档,无恢复点目标定义(阶段1 S1b 补) |
| G11 | 时间语义未加固 | 事件时间戳 = 墙钟 SystemTime::now(storage lib.rs:67);排序靠 (timestamp,id) 复合游标(events.rs:83),抗同秒但时钟回拨会让新事件 ts<旧事件 → as_of/断线补齐错乱(S1g) |
| G12 | 审计无物理 append-only 保证 | 审计表可被 UPDATE/DELETE 路径触碰(未验证),哈希链(S2d)需要存储层先钉死(S1h) |
| G13 | 无真实二进制冒烟 | 测试均进程内/HTTP 集成;无「真实二进制从零起→最小闭环」smoke test(S3e) |
| G14 | 无事务原子性测试 | Effect Executor 多表写是否同事务未验证,中途 kill 半写状态未知(S1c₀ 核心断言) |

## 复核结论(2026-08-22)

> 本节以 checkpoint `4569e94` 的代码、测试与 CI 证据为准;上方 G1-G14 保留为历史基线。

| # | 历史缺口 | 当前状态 | 复核证据 |
|---|----------|----------|----------|
| G1 | 首个 release tag | **未关闭** | `git tag --list` 为空 |
| G2 | 部署产物 | **已关闭** | `Dockerfile`、`docker-compose.yml` 存在;container/compose smoke 接入 CI |
| G3 | schema 迁移框架 | **已关闭** | `crates/oaip-storage/src/schema.rs:566-610` 实现 `ensure_schema` 版本迁移、迁移历史与 canonical schema 校验;up/down/drift/failure 测试齐备 |
| G4 | 性能基准 | **未关闭** | 未引入 criterion/iai,也无性能 bench 目录 |
| G5 | 备份/恢复 CLI | **已关闭** | `src/bin/oaip/commands.rs:24-39` 提供 `backup` / `restore`;`crates/oaip-storage/src/backup.rs:228-274` 覆盖快照校验后原子替换 |
| G6 | 健康与指标 | **已关闭** | `src/server/mod.rs:278-279` 暴露 `/healthz` 与 `/readyz`;`src/server/ops.rs:44-86` 暴露 `/metrics`;`tests/ops_http.rs:36-52` 钉公开探针行为 |
| G7 | 本体演进兼容性 | **已关闭** | `crates/oaip-abi/src/validation.rs` 加兼容规则;`tests/ontology_compatibility.rs` 钉演进行为 |
| G8 | 供应链审计 | **已关闭** | `.github/workflows/ci.yml` 执行 `cargo audit` |
| G9 | 资源边界 | **大部分关闭(按已定义边界)** | IO 只读/写满故障探针通过;超时、速率限制和 workspace 隔离有测试。生产容量级资源水位仍需运营数据 |
| G10 | RPO/RTO | **部分关闭** | 目标与流程已入档;仍缺生产规模真实数据的备份/恢复演练与实测 RPO/RTO 记录 |
| G11 | 时间语义 | **已关闭** | `crates/oaip-trace/src/lib.rs:63-79` 实现墙钟回拨下的单调时间护栏;重启时钟种子测试覆盖跨进程单调性 |
| G12 | 审计 append-only/哈希链 | **已关闭** | 存储层触发器阻断 UPDATE/DELETE;哈希链校验/导出有测试;`crates/oaip-storage/src/lib.rs:404-416` 暴露审计链健康 |
| G13 | 真实二进制冒烟 | **已关闭** | golden、container、compose smoke 均使用真实二进制并接入 CI |
| G14 | 事务原子性 | **已关闭** | IO 故障注入断言 world/trace 写入原子,失败路径不留下半写状态 |

## 架构债务复核结论(2026-08-22)

| 债 | 状态 | 复核结论 |
|----|------|----------|
| D1 Effect 双实现嫌疑 | **关闭** | kernel effect algebra 定义可逆语义,world executor 负责实际事件执行;职责分离确认,不是重复实现 |
| D2 Approval 双路径 | **关闭** | 运行时只读 `AppState.pending`;legacy `wb_approval` 路径已移除 |
| D3 投影职责未文档化 | **关闭** | kernel projection 与领域 projection 的层次职责已在模块头部说明 |
| D4 测试命名误导 | **关闭** | `v0_*` 伪 legacy 名称移除,测试改为按活跃契约命名 |

## 已确认事实(第二轮精修补充,2026-08-18 历史基线)
- libsql 0.9.30(storage 层无 backup 用法)→ S1c 先探 VACUUM INTO / SQLite backup API
- ObjectInstance 是 JSON 行(wb_ontology_object)→ ODL 加属性不改表;**本体演进是行为兼容问题不是 DDL 问题**(S1g)
- 时钟:SystemTime::now 墙钟 + (timestamp,id) 游标;回拨风险真实存在

## 架构债务证据(第三轮,用户点名 D;当前状态见「架构债务复核结论」)
| 债 | 证据 | 处置 |
|----|------|------|
| D1 Effect 双实现嫌疑 | src/world_exec/effect.rs(129 行,RelationEffectExecutor 被 compose/task.rs:288 活跃使用)vs crates/oaip-kernel/src/effect.rs(126 行,仅 Reversibility 被 simulation/projection.rs:13 用) | 阶段1A A1:合并或裁剪 |
| D2 Approval 双路径 | src/server/approvals.rs:3 自认「V1 读 ApprovalStore(planner 内存 + wb_approval 表);M1c 起读 AppState.pending」;tests/v0_3_approval.rs 注释同 | 阶段1A A2:确认旧路径无引用后删除(AGENTS.md 原则1) |
| D3 投影职责未文档化 | src/projection/mod.rs 424 行(领域规则 project_events/hydrate)vs crates/oaip-kernel/src/projection.rs 80 行(基础) | 阶段1A A3:职责边界文档化 |
| D4 测试命名误导 | tests/v0_1~v0_9 钉活跃语义(如 tc001 直接测 parse_odl,abi/odl.rs:80)非 legacy | 阶段1A A4:重命名/归类 |
| 排除:crats/src 分裂 | src/lib.rs:28-35 对 crates 是 pub use re-export(单一真相源) | 非债务,不动 |

## 黄金场景(用户点名 B,业务成功指标)
Project → Task → AgentRun 域(与 facade_v1/projection 测试语义同源);真实二进制+真实数据+真实执行器;
GS-1 定义 → GS-2 执行 → GS-3 治理 → GS-4 演化 → GS-5 恢复;每阶段收尾脚本化回归,最终 DoD D11。

## 待确认(2026-08-18 阶段 1 开工前,历史基线)
1. 生产形态:Local-First 单机(libsql 文件)vs 托管 libSQL —— 决定迁移框架与备份语义
2. 优雅停机现状:server 对 SIGTERM 的行为未验证(阶段 3 S3c)
3. coverage 实际值:CI 门禁 75% 是门槛,实际覆盖率未测(阶段 4 可顺带)

## 结论(2026-08-18 历史基线)
平台「能力正确性」已过测试关;「生产可用性」四大支柱(迁移/备份/部署产物/性能基线)处于文档态,无代码无演练。计划从数据平面(阶段1)开始补齐。

## 结论更新(2026-08-22)
平台已从「能力正确性原型」推进到「发布前收口」状态:数据平面、治理控制面、部署冒烟和审计链均有自动化证据。
剩余不是功能正确性问题,而是发布与运营闭环:打首个 release tag,用生产规模真实数据完成备份/RPO/RTO 演练,
再建立性能/负载基线和 SLA。另有局部工程质量债:`src/automation.rs` 约 1859 行,应拆分到独立自动化运行时模块。
