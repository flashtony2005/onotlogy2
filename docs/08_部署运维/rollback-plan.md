# 回滚方案
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-08-005 |
| 所属模块 | OpenAIP-OS-Ops |
| 版本 | V1.0 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
规定各类故障的回滚步骤与决策条件，保障可快速恢复。

## 2. 业务/技术背景
依赖 deploy-steps.md 与 version-changelog.md。

## 3. 核心规则 & 约束（重点）
- [强制] 每次上线必须配备回滚方案，且与 changelog 版本对应。
- [强制] 回滚需含：触发条件、操作步骤、数据兼容说明、验证方式。

## 4. 详细设计/内容
### 4.1 回滚触发条件
### 4.2 回滚步骤
1. 停止服务，确认没有进程继续打开数据库文件。
2. 保留当前故障版本日志、`/metrics` 快照和数据库文件，不覆盖证据。
3. 切回上一稳定 tag 或上一版本 release binary。
4. **必须恢复升级前快照**；禁止让旧 binary 直接打开新 schema 数据库。
5. 启动旧版本，依次验证 `/healthz`、`/readyz`、对象查询、审计查询和黄金场景。
6. 记录实际 RTO、恢复点时间（RPO）和复核人。

### 4.3 数据兼容说明
- schema 只保证旧版本数据库向新版本升级；
- 新版本数据库不能直接被旧版本打开，旧版本必须返回 `WB2101`；
- 回滚数据源是升级前的停止态一致性快照，不能复制运行中的主库/WAL/SHM 拼接；
- 升级前快照必须在放量前完成并留存校验和。

### 4.4 可执行演练

```bash
OAIP_BASE_BIN=/srv/releases/previous/oaip \
OAIP_CANDIDATE_BIN=/srv/releases/candidate/oaip \
make ops-rehearsal
```

完整断言与实测记录见 [upgrade-rollback-rehearsal.md](upgrade-rollback-rehearsal.md)。

## 5. 风险点 & 注意事项
- 数据库不兼容会导致回滚失败，需提前评估。

## 6. 关联文档
- [../08_部署运维/deploy-steps.md](../08_部署运维/deploy-steps.md)
- [../09_迭代与版本/version-changelog.md](../09_迭代与版本/version-changelog.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成 |
