# Git 提交与分支规范
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-06-003 |
| 所属模块 | OpenAIP-OS-Dev |
| 版本 | V1.0 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
强制分支模型与 Commit 格式，保证提交可追溯。

## 2. 业务/技术背景
被所有开发活动遵守。

## 3. 核心规则 & 约束（重点）
- [强制] 分支模型：main（生产稳定，禁直接 push）、dev（开发主）、feature/xxx、hotfix/xxx、release/vx.x.x。
- [强制] Commit 格式：<type>(模块): 简短描述；type ∈ feat/fix/docs/style/refactor/test/chore。
- [强制] 任何 MD 修改提交须 1 人以上评审。

## 4. 详细设计/内容
### 4.1 分支模型
| 分支 | 用途 | 约束 |
|------|------|------|
| main | 生产 | 仅 merge/release |
| dev | 开发 | 合入 feature |
| feature/xxx | 功能 | 从 dev 拉 |
| hotfix/xxx | 紧急修复 | 从 main 拉 |
| release/vx.x.x | 发布 | 从 dev 拉 |

### 4.2 Commit 示例
```
feat(user): 新增用户手机号绑定接口
fix(task): 修复任务状态流转 bug
docs: 更新 workbuddy 数据库表结构文档
```


## 5. 风险点 & 注意事项
- main 直接 push 会破坏生产稳定性，需保护分支。

## 6. 关联文档
- [../01_项目总览/project-constraint.md](../01_项目总览/project-constraint.md)
- [../09_迭代与版本/version-changelog.md](../09_迭代与版本/version-changelog.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成 |

