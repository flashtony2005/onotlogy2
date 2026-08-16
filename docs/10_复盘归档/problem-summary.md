# 问题复盘汇总
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-10-001 |
| 所属模块 | OpenAIP-OS-Retro |
| 版本 | V1.0 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
记录线上/流程重大问题与根因，沉淀组织经验。

## 2. 业务/技术背景
依赖 bug-record-template.md 与 meeting-minutes-template.md。

## 3. 核心规则 & 约束（重点）
- [强制] 问题须含：现象、影响、根因、改进项、责任人、截止。
- [建议] 按时间倒序归档。

## 4. 详细设计/内容
### 4.1 问题记录
| 日期 | 问题 | 根因 | 改进项 | 责任人 |
|------|------|------|--------|--------|
| 2026-08-14 | H1 无 Saga 补偿（planner 失败不回滚） | V1 run_goal 设计取舍 | ✅ V2 Kernel 补偿链（P1 逆序补偿 + ESCALATED）接管 | lushaohui |
| 2026-08-14 | H3 link TOCTOU 双插 | 无事务检测+插入 | ✅ 事务串行化（storage/relations.rs + tc012b） | lushaohui |
| 2026-08-14 | H4 乐观锁形同虚设 | 读-改-写无 CAS | ✅ version 原子递增（storage/objects.rs + 并发测试） | lushaohui |
| 2026-08-14 | M4 明文 token 落库 | 会话表直接存令牌 | ✅ SHA-256 哈希入库（auth.rs hash_token + tc_auth_k） | lushaohui |
| 2026-08-14 | M7 迁移脆弱（单常量+吞错） | 无版本化迁移 | ✅ SCHEMA_VERSION 8→9 版本化迁移（MIGRATIONS + 事务回滚） | lushaohui |
| 2026-08-16 | 双轨并行（V1 planner ∥ V2 loop） | 无退役日期的并行迁移 | ✅ M1d 物理删除，AgentLoop 唯一路径（b75e7b0） | lushaohui |
| 2026-08-16 | 三套 Learn 口径并存 | V1/V2 演化各自为政 | ✅ M1d 删除 evolution.rs + 三表；Gate/rule/skill 一层口径 | lushaohui |
| 2026-08-16 | 审计查询缺失（REQ-018 可审计承诺断链） | V1 审计表随门面删除 | ✅ M2 /audit 端点读事实轨（b6a80d5） | lushaohui |


## 5. 风险点 & 注意事项
- 只记录现象不挖根因无法防止复发。

## 6. 关联文档
- [../07_测试文档/bug-record-template.md](../07_测试文档/bug-record-template.md)
- [../10_复盘归档/optimization-record.md](../10_复盘归档/optimization-record.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成 |

