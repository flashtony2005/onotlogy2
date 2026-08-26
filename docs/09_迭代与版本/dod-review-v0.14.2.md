# V0.14.2 生产可用 DoD 全量复核表

复核时间：2026-08-26。本文只记录可执行证据；“待运营环境补充”不是工程验收失败，
但会作为对外承诺边界显式声明。

| # | 验收项 | 判定 | 可执行证据 | 当前结论与边界 |
|---|--------|------|------------|----------------|
| D1 | 数据不丢 | ✅ | `crates/oaip-storage/src/lib.rs::crash_injection_preserves_atomic_world_and_recovers_runtime_states` 注入进程级 crash；`tests/runtime_persistence.rs` 覆盖 run 生命周期、重启重建和 running → interrupted；Golden GS-5 覆盖 kill 后世界一致与挂起任务恢复。 | 任意注入点不留下半写世界；事件日志、投影和运行三态可在重启后恢复。 |
| D2 | 变更可审计 | ✅ | `tests/invariants.rs` 的 i6/i10；`tests/audit_chain.rs` 断言追加链、导出稳定和审计失败回滚；存储触发器阻断审计 UPDATE/DELETE。 | 世界、审批、Fabric 和治理变化都有审计路径，物理 append-only 和哈希链校验生效。 |
| D3 | 安全 fail-closed | ✅ | `tests/auth_rbac.rs` 覆盖无 token、错 token、禁用账号、角色钳制、限流和 key rotation；`tc_auth_*` 验证会话哈希与轮换撤销；plugin 测试覆盖权限/secret 隔离。 | 无认证、无授权或失效凭据拒绝访问；密钥可轮换且旧 session 失效。 |
| D4 | 可迁移可回滚 | ✅ | `crates/oaip-storage/src/schema.rs` 测试覆盖 canonical drift 拒绝、每个迁移 up/down 往返、失败事务回滚；`docs/08_部署运维/upgrade-rollback-rehearsal.md` 记录 schema 19→20 和 v0.14.0→v0.14.x 同 schema 真实 binary 演练。 | schema 20 迁移历史完整；旧 binary 拒绝新 schema 并返回 `WB2101`，升级前快照可恢复。 |
| D5 | 一条命令运维 | ✅ | `make ops-smoke`、`make container-smoke`、`make compose-smoke`、`make backup-rehearsal`、`make release-manifest`；Compose 入口见 deploy steps。 | 启动、探针、停止、备份、恢复和发布清单均为命令级操作；S5a Compose 持久化演练通过。 |
| D6 | 升级回滚演练过 | ✅ | CI 执行 `OAIP_BASE_REF=HEAD^ make ops-rehearsal`；`upgrade-rollback-rehearsal.md` 记录 19→20、v0.14.0→修复候选、降级拒绝和快照回滚。 | 至少一次真实 v1→v2→回滚路径已验证；同 schema 直读回滚也单独演练。 |
| D7 | 性能有基线 | ✅ | Criterion 四条核心路径、HTTP/WS 负载基线和 Reasoner scale 表入档 `performance-baseline.md`；CI 执行 `reasoner-scale-ci`。 | 5k 推理阶段本机约 211–233ms；本机红线 1000ms，CI 预算 3000ms。数字非跨硬件合同。 |
| D8 | 发布有产物 | ✅ | Dockerfile、Compose、release binary、UI build 和 `target/release-manifest.json` 由脚本生成；manifest 默认构建并绑定仓库 `target/release/oaip` 的 version/schema/commit/SHA/image，脏树强制 candidate。 | 正式产物由干净工作树 + `v0.14.2` tag + release manifest 锚定。 |
| D9 | 资源与供应链边界 | ✅ | `crates/oaip-storage/src/lib.rs::io_faults_preserve_world_and_trace_atomicity` 注入磁盘满/只读；workspace/timeout/rate-limit/process-group 有契约测试；CI 执行 cargo audit 白名单门禁。 | IO 故障不产生半写状态；执行资源、网络和供应链审计有自动化边界。 |
| D10 | 备份有 RPO/RTO 语义 | ✅/⚠️ | `backup-restore.md` 定义 RPO 15 分钟、RTO 5 分钟；`make backup-rehearsal BACKUP_TARGET_MIB=1016` 实测 31,226 对象、1020 MiB、总 RTO 15.701s；Compose 重启读回验证 volume 持久化。 | 工程/单机演练关闭。生产 RPO 仍需目标环境的 15 分钟外部调度器成功记录、失败告警和真实写入窗口恢复证据。 |
| D11 | 黄金场景业务验收 | ✅ | `scripts/golden-scenario.sh` 用真实 binary 和确定性 process executor 跑 GS-1~GS-5：ODL 定义、动作执行、审批/审计、本体演化、kill 恢复；CI 接入 `make golden`。 | 业务链路不是孤立函数测试；真实 HTTP/WS/存储/进程边界均被脚本覆盖。 |
| D12 | 架构债清零 | ✅ | `findings.md` 复核 Effect 双实现、Approval 双路径、投影职责和伪 legacy 测试均已关闭；`tests/invariants.rs` 钉 I1/I2/I5/I6/I7 等核心不变量。 | 无已知双路径或“伪 legacy”架构债；新债务需另开登记而不是混入本表。 |

## 总判定

D1–D12 工程验收全部关闭，V0.14.2 可以作为**单机 Local-First 生产核心 GA**发布。

对外使用时必须同时声明两个运营前提：

1. 生产 RPO 目标要求外部调度器每 15 分钟执行备份，并留存成功、失败告警和真实写入窗口恢复证据。
2. 性能/容量 SLA 必须在目标硬件、数据规模和部署拓扑重新测量；仓库内数字是回归基线，不是通用合同。
