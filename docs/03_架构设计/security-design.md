# 安全设计
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-03-003 |
| 所属模块 | OpenAIP-OS-Architecture |
| 版本 | V1.1 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
规定权限模型、数据加密与防攻击策略。

## 2. 业务/技术背景
依赖 architecture-total.md 与 05_接口文档。

## 3. 核心规则 & 约束（重点）
- [强制] 权限模型必须文档化（如 RBAC），接口标注权限要求。
- [强制] 敏感字段（密码、令牌）必须加密存储，禁止明文。
- [强制] 防注入、防越权、防重放等攻击面须逐条列出防护手段。

## 4. 详细设计/内容
### 4.1 权限模型
MVP 采用 RBAC，角色与 `agent-observation-design.md` 三层视图对齐；Kernel 自身无业务权限，仅 Agent 动作受 Policy 约束。

| 角色 | 可见范围 | 可执行 |
|------|----------|--------|
| Executive（管理者） | Mission / Progress / Decision / Risk 摘要 | 审批 Human-Approval 动作 |
| Operator（管理员） | 运行时状态 + Pipeline 进度 + Timeline | 触发运行、干预 |
| Developer（开发者） | Trace 全量（Input / Ontology Context / Planner Output / Tool Call） | 触发运行、调试 |
| System（运行时进程） | 自身写权限 | 写 `wb_trace_event`、调 Policy |

- [强制] 所有写类轨迹 / 触发运行接口必须登录且角色 ≥ Operator；Executive 仅可读摘要（见 `api-agent-trace.md` 4.4 `view=executive`）。
- [强制] Agent 动作在执行前必须经 `Policy.check()`，被拒返回 WB2005，敏感动作（如 createSalesTask）需 Human Approval。

### 4.2 数据加密策略
| 字段 | 加密方式 | 存储位置 |
|------|----------|----------|
| wb_user_account.username | 明文（业务标识，非凭据） | Turso |
| 用户口令（若引入登录） | bcrypt 单向哈希，禁止明文/可逆 | Turso |
| Pi Provider Key / LLM Token | 不落库，经环境变量或密钥管理注入进程 | 进程内存 / Secret Manager |
| wb_ontology_object.data（JSON） | 默认明文；含 PII 的 Object 类型须在 ObjectDef 标注 `sensitive`，存储层启用字段级加密 | Turso |
| wb_trace_event.reasoning/input/output | 明文（审计需要可解释） | Turso |

### 4.3 攻击面防护
| 风险 | 防护手段 |
|------|----------|
| SQL 注入 | 全部经 libSQL 参数化查询，禁止字符串拼接 SQL；DDL 仅来自 `sql-script.md`，运行时不接受外部 DDL |
| 越权访问 | `objectRef` 过滤在服务端强制生效；角色校验中间件统一拦截；Executive 视野由后端裁剪，不依赖前端隐藏 |
| 重放攻击 | WebSocket 握手须持有效登录令牌；`runId` 幂等去重，重复触发返回已有 run |
| Prompt 注入 | Pi（Cognitive Engine）输出视为不可信数据，工具调用经 `Policy` 沙箱与白名单，禁止 Agent 直接执行任意系统命令 |
| 逻辑删除绕过 | 所有查询默认附加 `is_delete = 0`，物理删除禁用 |


## 5. 风险点 & 注意事项
- 安全设计遗漏直接导致线上事故，需安全评审。

## 6. 关联文档
- [../03_架构设计/architecture-total.md](../03_架构设计/architecture-total.md)
- [../05_接口文档/api-restful-spec.md](../05_接口文档/api-restful-spec.md)
- [../05_接口文档/error-code.md](../05_接口文档/error-code.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成 |
| V1.1 | lushaohui | 2026-08-11 | 填充 4.1 RBAC（Executive/Operator/Developer/System）、4.2 数据加密策略、4.3 攻击面防护（注入/越权/重放/Prompt 注入/逻辑删除） |

