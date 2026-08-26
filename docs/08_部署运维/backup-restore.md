# 数据库备份与恢复

## 适用范围

当前生产形态为单机 Local-First：`libSQL` 本地文件 + 单进程服务。`oaip backup`
和 `oaip restore` 只操作本地数据库文件，不接受远程 Turso/libSQL URL。

## RPO / RTO

| 指标 | 目标 | 语义 |
|---|---:|---|
| RPO | 15 分钟 | 生产环境由外部定时任务每 15 分钟执行一次 `oaip backup`；最多丢失最近一次成功快照之后的写入 |
| RTO | 5 分钟 | 数据库文件不超过 1 GiB、备份文件可用且服务已停止时，从选择快照到恢复后健康检查通过的目标时间；首次演练前不宣称已达标 |

快照是显式运维动作，不由应用后台自动触发。备份目标已存在时命令拒绝覆盖，
因此定时任务必须使用带时间戳的目标文件名，并由保留策略清理过期快照。

## 备份

```bash
oaip --db /var/lib/oaip/openaip.db backup \
  /var/backups/oaip/openaip-$(date -u +%Y%m%dT%H%M%SZ).db
```

实现使用数据库引擎的一致性快照写入同目录临时文件，再原子改名；不会通过复制
正在运行的主库文件来拼接 WAL/SHM 状态。备份成功后应检查文件存在并记录快照时间。

## 恢复

1. 停止 OpenAIP-OS 服务，确保没有其他进程打开目标数据库。
2. 选择恢复点并执行：

   ```bash
   oaip --db /var/lib/oaip/openaip.db restore \
     /var/backups/oaip/openaip-20260818T120000Z.db
   ```

3. 命令会先校验快照的核心表和当前 canonical schema，再以同目录文件替换安装；
   非法快照不会触碰目标库。
4. 启动服务，执行健康检查、对象查询、审计查询和黄金场景恢复步验证。

恢复不覆盖源备份，不对旧 WAL/SHM 旁车文件复用；恢复前必须停服务，否则并发写入
不属于本工具的保证范围。

## 可重复演练

使用真实二进制执行隔离目录内的端到端演练：启动服务、经公开 HTTP 写入确定性对象、
停止服务、执行 `backup`、恢复到新库、再次启动服务并校验 `/readyz`、审计链、全量对象计数、
marker、抽样 ID 和 payload 长度。结果输出为机器可读 JSON：

```bash
make backup-rehearsal BACKUP_TARGET_MIB=64 BACKUP_PAYLOAD_KIB=16 BACKUP_CONCURRENCY=8
```

脚本入口：`scripts/backup-rpo-rto-rehearsal.py`。默认报告为
`target/backup-rehearsal.json`；正式发布证据应复制到受保留的发布目录，不要只依赖本地
`target/`。

### 当前实测冒烟（2026-08-26，Apple M4 本机）

| 项 | 结果 |
|---|---|
| 二进制 | oaip 0.14.0，SHA-256 `4c28e4a6f5ab267aedbb0f1c78e227094cbfe2e0fc11ee177d07a10396d1ff9d` |
| 数据规模 | 目标 64 MiB；写入 2,048 个对象；主库 + sidecar 实测 67.203125 MiB |
| 备份 | 67.140625 MiB，0.249s |
| 恢复 CLI | 0.214s |
| 恢复后 readyz | 0.290s |
| 总 RTO | 0.505s（远小于 300s 目标） |
| 数据校验 | 全量计数、marker、8 个抽样 ID 和 payload 长度全部一致 |
| 审计 | `/readyz` 为 `ready` 且 `auditChain=valid`；恢复库审计可导出 |

### 当前实测生产规模档（2026-08-26，Apple M4 本机）

| 项 | 结果 |
|---|---|
| 二进制 | oaip `0.14.0`，Git `cff1cd9`，SHA-256 `88aa3af2cb55170cfd5846c1a09b77eb2498f6734bd045126b508738794f4ce0` |
| 数据规模 | 种子目标 1016 MiB；写入 31,226 个对象；主库 + sidecar 实测 1020.292969 MiB（≤1 GiB 上限） |
| 备份 | 1019.285156 MiB，9.636102s，SHA-256 `a82526285e7df2b0b6f215f508e5b3c58545bae4f6251b188b16c7309397a273` |
| 恢复 CLI | 5.996403s |
| 恢复后 readyz | 9.704044s |
| 总 RTO | 15.701049s（≤300s 目标） |
| 数据校验 | 全量计数 31,226、marker、8 个抽样 ID 和 31,226 行 payload 长度全部一致 |
| 审计 | `/readyz` 为 `ready` 且 `auditChain=valid`；恢复库审计已导出 |
| 报告 | 本机执行时归档 `target/backup-rehearsal.json`；正式发布必须复制到受保留目录 |

这次是**停止服务后的静默快照完整性/RTO 冒烟**。它不测量定时备份间隔内的写入丢失，
因此不能单独宣称生产 RPO ≤ 15 分钟；该结论仍需要外部调度器每 15 分钟成功备份、失败告警、
以及一次真实窗口内写入后的恢复证据。

## 当前限制

- 备份/恢复尚未覆盖远程数据库。
- RPO/RTO 是运维目标；1 GiB 规模档端到端静默演练已通过，但外部定时备份失败告警与真实
  写入窗口的 RPO 证据仍未完成。
- 迁移 `down` 的历史数据可恢复性仍需单独评审；恢复工具只接受 schema 校验通过的快照。
