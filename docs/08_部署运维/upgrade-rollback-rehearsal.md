# 升级与回滚演练记录

## 1. 目的

S3d 不把“旧二进制能够启动”当作回滚证据，而是验证完整的真实路径：

1. 旧版本二进制创建并写入数据库；
2. 新版本二进制打开同一数据库，执行真实 schema 升级；
3. 新版本执行一致性备份与恢复；
4. 旧版本面对新 schema 明确拒绝启动；
5. 使用升级前快照恢复后，旧版本重新读取原有对象和审计数据。

这一定义避免了把“二进制降级”和“数据回滚”混为一谈。

## 2. 执行入口

候选 release binary 必须先构建：

```bash
cargo build --release --bin oaip
make release-manifest
```

发布清单中的 binary SHA-256、Git commit 和 schema 版本必须与演练记录一起留存。

使用已有旧版本二进制：

```bash
OAIP_BASE_BIN=/srv/releases/previous/oaip \
OAIP_CANDIDATE_BIN="$PWD/target/release/oaip" \
make ops-rehearsal
```

在只有 Git ref、没有预装旧 binary 的环境中，可以由脚本从 ref 构建隔离的旧版本：

```bash
OAIP_BASE_REF=v0.9.0 \
OAIP_CANDIDATE_BIN="$PWD/target/release/oaip" \
make ops-rehearsal
```

`OAIP_BASE_BIN` 与 `OAIP_BASE_REF` 至少提供一个。脚本拒绝默认使用候选 binary，
避免产生没有意义的“自测回滚”。

## 3. 验证断言

| 阶段 | 断言 |
|---|---|
| V1 数据种子 | 旧 binary 能写入 `Customer/001`、`Customer/002` |
| V1 → V2 | 新 binary 能打开同一文件，并完成 schema 迁移 |
| V2 快照 | 新 binary 的 `backup` 生成不可覆盖的快照 |
| V2 恢复 | 新 binary 的 `restore` 后对象与审计事件仍存在 |
| 降级保护 | 旧 binary 打开 V2 数据必须失败并返回 `WB2101` |
| V2 → V1 | 复制升级前停止态快照后，旧 binary 能重新读取原对象 |

当前 schema 是单向演进的：**不支持直接让旧 binary 打开新 schema**。因此生产回滚
必须先停止服务，再恢复升级前快照，最后启动旧 binary；禁止只替换镜像而保留新 schema
数据库。

## 4. 本轮实测记录

| 项 | 结果 |
|---|---|
| 执行日期 | 2026-08-19 |
| 旧 binary | `HEAD^`，版本 `0.9.0`，SHA-256 `9ae5ac77fa3ada7d8aba13ecffd361bdf58fb8f380854c28e2d3d4b8b268dea1` |
| 新 binary | 工作区 release binary，版本 `0.9.0`，SHA-256 `53c8b16633fcde2c7bdcee7d048c273508eeea44d9cbb692d54e91c85134771d` |
| V1 → V2 | PASS；旧 schema 10 自动迁移至当前 schema 13 |
| V2 backup/restore | PASS；恢复后的对象与 `phase3_upgrade_verified` 审计事件可读 |
| 旧 binary 直读 V2 | PASS；按预期拒绝，错误码 `WB2101` |
| V2 → V1 | PASS；恢复升级前快照后旧 binary 可读 |
| 本地停止态恢复 RTO | 0 秒（小型临时库的下界测量，不作为生产承诺） |

该记录使用实际编译的旧/新 release binary 和实际 libSQL 文件，不使用 mock server。
生产 RPO/RTO 仍需用接近生产容量的数据和真实备份介质单独测量。

## 5. 发布前必须留存的证据

- 旧 binary 的版本号、Git ref、SHA-256；
- 新 binary 的版本号、Git ref、SHA-256；
- 升级前快照路径、创建时间、文件大小、SHA-256；
- `/healthz`、`/readyz`、`/metrics` 的验证结果；
- 实际恢复耗时和恢复后对象/审计抽样结果；
- 操作人、复核人和最终是否创建 release tag。

## 6. 关联文档

- [backup-restore.md](backup-restore.md)
- [rollback-plan.md](rollback-plan.md)
- [deploy-steps.md](deploy-steps.md)
