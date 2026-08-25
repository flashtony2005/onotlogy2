# 全局错误码
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-05-003 |
| 所属模块 | OpenAIP-OS-API |
| 版本 | V1.3 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
集中定义全局错误码（WB 前缀），接口文档统一引用。

## 2. 业务/技术背景
被所有接口文档与代码引用。

## 3. 核心规则 & 约束（重点）
- [强制] 错误码前缀 WB，格式 WBxxxx（如 WB1001）。
- [强制] 每个错误码含：含义、触发场景、建议处理。
- [强制] 代码中的错误码常量必须与本文档一一对应。

## 4. 详细设计/内容
### 4.1 错误码表
| 错误码 | 含义 | 触发场景 | 建议处理 |
|--------|------|----------|----------|
| WB0000 | 成功 |          |          |
| WB1001 | 参数错误 | 入参校验失败 | 前端提示 |
| WB1002 | 未登录 | 缺失令牌 | 跳转登录 |
| WB2001 | Agent 不存在 | agentId 查无对应 Agent | 检查 agentId |
| WB2002 | Trace 不存在 | trace id 查无对应轨迹事件 | 检查 trace id |
| WB2003 | 运行不存在 | runId 查无对应 Agent 运行 | 检查 runId |
| WB2004 | 认知引擎不可用 | Pi（Cognitive Engine）未就绪 / 进程崩溃 / LLM 鉴权失败 | 检查 Pi 进程与 Provider 配置 |
| WB2005 | Policy 拒绝 | Action 被 Policy 引擎拒绝，需 Human Approval | 转人工审批 |
| WB2006 | 轨迹缺对象绑定 | `TraceEvent.objectRefs` 为空，违反可解释性约束 | 写入拦截，补齐 object_refs |
| WB2007 | 重复关系边 | `link` 相同 (source, edge, target) 已存在（未删） | 去重或先 `unlink` 再重连 |
| WB2205 | 项目知识版本绑定无效 | 项目 ODL 与请求版本 Runtime IR 不一致、版本缺少 Runtime IR，或绑定版本与 live world 冲突 | 先修正项目 ODL 或选择经过治理且可执行的版本 |
| WB2101 | 旧 schema 不兼容 | 本地库缺 `wb_meta.schema_version`（旧列 object_type/from_id/to_id/edge_type）或版本号不匹配，`CREATE TABLE IF NOT EXISTS` 无法迁移 | 运行 `oaip db reset` 删除并重建数据库 |


## 5. 风险点 & 注意事项
- 错误码重复或含义漂移会导致前端无法准确提示。

## 6. 关联文档
- [../05_接口文档/module-template-api.md](../05_接口文档/module-template-api.md)
- [../04_数据库设计/data-dict.md](../04_数据库设计/data-dict.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成 |
| V1.1 | lushaohui | 2026-08-11 | 新增 Agent 模块错误码 WB2001~WB2006（对应 api-agent-trace.md） |
| V1.2 | lushaohui | 2026-08-11 | 新增 WB2007（重复关系边，对应 V0.2 Relation 引擎） |
| V1.3 | lushaohui | 2026-08-12 | 新增 WB2101（旧 schema 不兼容，对应 `oaip db reset` 迁移） |
