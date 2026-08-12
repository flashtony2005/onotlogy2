# 表结构设计
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-04-002 |
| 所属模块 | WorkBuddy-DB |
| 版本 | V1.3 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
定义所有表的字段、类型、索引、约束与注释，是建表与代码的权威依据。

## 2. 业务/技术背景
依赖 db-total-erd.md 与 data-dict.md。

## 3. 核心规则 & 约束（重点）
- [强制] 每张表必须包含：表中文名、用途、主键、索引、唯一约束、字段注释、数据类型、是否非空、默认值。
- [强制] 表名规则：wb_模块名_表名，下划线分隔，全小写。示例：wb_user_account、wb_task_record。
- [强制] 统一字段：id 主键（系统表 `INTEGER AUTOINCREMENT`；领域对象表 `TEXT` 语义ID，如 `customer_001` / `tr_xxx`）、create_time、update_time、is_delete 逻辑删除。
- [强制] 枚举全部抽离到 data-dict.md，代码与文档枚举值完全一一对应。
- [强制] 所有修改必须更新修订日志；DDL 脚本单独存放 sql-script.md，禁止直接在线改库。

## 4. 详细设计/内容
### 4.1 表清单
| 表名 | 中文名 | 用途 |
|------|--------|------|
| wb_user_account | 用户账户 | 系统内部账号 |
| wb_ontology_object | 本体对象实例 | Everything is Object：每类业务对象统一存储，禁止按业务建独立表（V0.1） |
| wb_relation_edge | 关系边 | 世界连接，支撑图查询（V0.2） |
| wb_trace_event | 统一事件轨迹 | 存储 TraceEvent（world 生命周期 + cognitive 认知），支撑三层观察界面与 Trace API |
| wb_memory | 记忆沉淀 | Agent 自治基础（V0.5 · REQ-016）：经验按 `action.<name>` 去重累加 hits，下一轮 run 经 MemoryRecall 召回，形成 Action→Memory→下次 Plan 闭环 |
| wb_pattern | 进化模式 | Evolution Engine（BL-003 · REQ-017）：由 wb_memory 经验提炼的 Pattern（hits≥2 才成 Pattern），携带可追溯 run_ids |
| wb_proposal | 进化提案 | Evolution 提案（Pattern→Proposal），状态 Proposed/Validated/Rejected，携带来源 pattern_label |
| wb_learned | 沉淀知识 | Evolution 最终知识（仅 Validated 才沉淀），携来源 proposal_id 与 `auto_approve_enabled`（可撤销资格），**不修改 Policy、不绕过审批**（Learn≠Permission）；REQ-018 自动放行资格存于此 |
| wb_approval_audit | 放行审计 | REQ-018 自动放行审计：每次自动放行/资格切换落库（decision=`auto_approve`/`auto_approve_enabled`/`revoke_disabled`），可追溯至 LearnedKnowledge + run |

### 4.2 表结构示例：wb_user_account
- 用途：
- 主键：id
- 唯一约束：uk_username(username)
- 索引：idx_create_time(create_time)

| 字段 | 类型 | 非空 | 默认 | 注释 | 枚举(若有) |
|------|------|------|------|------|------------|
| id | integer | 是 | - | 主键 | - |
| create_time | text | 是 | CURRENT_TIMESTAMP | 创建时间 | - |
| update_time | text | 是 | CURRENT_TIMESTAMP | 更新时间 | - |
| is_delete | integer | 是 | 0 | 逻辑删除 | 见 data-dict |
| username | text | 是 | - | 用户名 | - |

### 4.3 表结构：wb_trace_event
- 用途：统一事件轨迹存储，承接 Agent Runtime 经 Event Bus 写出的 `TraceEvent`（world 与 cognitive 同源），支撑三层观察界面（Executive / Operator / Developer View）与 Trace API（见 `api-agent-trace.md`）。
- 主键：id
- 唯一约束：无（事件可重复写入）
- 索引：idx_trace_run(run_id)、idx_trace_agent(agent_id)、idx_trace_type(event_type)、idx_trace_time(timestamp)、idx_create_time(create_time)

| 字段 | 类型 | 非空 | 默认 | 注释 | 枚举(若有) |
|------|------|------|------|------|------------|
| id | text | 是 | - | 语义轨迹ID（如 tr_abc123），与 API DTO.id 直接对应 | - |
| create_time | text | 是 | CURRENT_TIMESTAMP | 记录创建时间 | - |
| update_time | text | 是 | CURRENT_TIMESTAMP | 记录更新时间 | - |
| is_delete | integer | 是 | 0 | 逻辑删除 | 见 data-dict |
| kind | text | 是 | - | 事件大类 | world,cognitive（见 data-dict） |
| run_id | text | 否 | - | 认知运行ID（kind=cognitive 必填） | - |
| agent_id | text | 否 | - | Agent ID（kind=cognitive 必填） | - |
| event_type | text | 是 | - | 事件类型 | 见 data-dict |
| object_refs | text | 否 | - | 绑定的 Ontology 对象引用，JSON 数组，可解释性根基；反查经 wb_ontology_object.id | - |
| reasoning | text | 否 | - | 推理过程（kind=cognitive 专有） | - |
| input | text | 否 | - | 输入载荷 JSON | - |
| output | text | 否 | - | 输出载荷 JSON | - |
| confidence | real | 否 | - | 置信度 0~1 | - |
| timestamp | text | 是 | - | 事件发生时间（业务时间） | - |

### 4.4 表结构：wb_ontology_object
- 用途：本体对象实例存储，Everything is Object 的落地；每类业务对象统一存此表，禁止按业务建独立表。
- 主键：id
- 唯一约束：无（id 即业务语义主键）
- 索引：idx_obj_type(type)、idx_obj_create(create_time)

| 字段 | 类型 | 非空 | 默认 | 注释 | 枚举(若有) |
|------|------|------|------|------|------------|
| id | text | 是 | - | 语义对象ID（如 customer_001） | - |
| create_time | text | 是 | CURRENT_TIMESTAMP | 创建时间 | - |
| update_time | text | 是 | CURRENT_TIMESTAMP | 更新时间 | - |
| is_delete | integer | 是 | 0 | 逻辑删除 | 见 data-dict |
| type | text | 是 | - | 对象类型（对应 ODL object 名，如 Customer） | - |
| data | text | 是 | - | 对象数据 JSON（由 ObjectDef 驱动，禁止 match 硬编码） | - |
| version | integer | 是 | 1 | 乐观锁版本号，update 自增 | - |

### 4.5 表结构：wb_relation_edge
- 用途：世界连接（关系边），支撑图查询；V0.2 引入。
- 主键：id
- 唯一约束：无
- 索引：idx_edge_source(source)、idx_edge_target(target)、idx_edge_rel(relation)

| 字段 | 类型 | 非空 | 默认 | 注释 | 枚举(若有) |
|------|------|------|------|------|------------|
| id | text | 是 | - | 语义边ID（如 e_001） | - |
| create_time | text | 是 | CURRENT_TIMESTAMP | 创建时间 | - |
| update_time | text | 是 | CURRENT_TIMESTAMP | 更新时间 | - |
| is_delete | integer | 是 | 0 | 逻辑删除 | 见 data-dict |
| source | text | 是 | - | 源对象ID（FK → wb_ontology_object.id） | - |
| relation | text | 是 | - | 关系名（如 purchased / owns） | - |
| target | text | 是 | - | 目标对象ID（FK → wb_ontology_object.id） | - |
| data | text | 否 | - | 关系附载 JSON（可选） | - |

### 4.6 表结构：wb_memory
- 用途：Agent 自治的记忆沉淀（V0.5 · REQ-016）；经验按 `label` 去重并累加 `hits`，下一轮 run 经 `recall_all` 召回复用，形成 Action→Memory→下次 Plan 闭环。
- 主键：label
- 唯一约束：无（label 即语义主键，幂等 upsert）
- 索引：idx_memory_hits(hits)

| 字段 | 类型 | 非空 | 默认 | 注释 | 枚举(若有) |
|------|------|------|------|------|------------|
| label | text | 是 | - | 记忆标签（如 action.retain / #RetentionPattern） | - |
| payload | text | 是 | - | 经验载荷 JSON | - |
| hits | integer | 是 | 1 | 命中/复用次数，自学习沉淀指标 | - |
| created_at | text | 是 | CURRENT_TIMESTAMP | 创建时间 | - |
| updated_at | text | 是 | CURRENT_TIMESTAMP | 更新时间 | - |

### 4.7 表结构：wb_pattern
- 用途：Evolution Engine（BL-003 · REQ-017）的模式层；由 wb_memory 经验经 `mine_patterns` 提炼，hits≥2（PATTERN_MIN_HITS）才成 Pattern，`confidence = min(1, hits/5)`。
- 主键：label
- 唯一约束：无
- 索引：无（label 主键已覆盖）

| 字段 | 类型 | 非空 | 默认 | 注释 | 枚举(若有) |
|------|------|------|------|------|------------|
| label | text | 是 | - | 模式标签（如 pattern.action.retain） | - |
| action | text | 是 | - | 关联动作名 | - |
| support | integer | 是 | 0 | 支持度 = 来源记忆 hits | - |
| confidence | real | 是 | 0.0 | 置信度 min(1, hits/5) | - |
| run_ids | text | 是 | '[]' | 贡献来源 run（JSON 数组，可追溯至 AgentRun） | - |
| created_at | text | 是 | CURRENT_TIMESTAMP | 创建时间 | - |
| updated_at | text | 是 | CURRENT_TIMESTAMP | 更新时间 | - |

### 4.8 表结构：wb_proposal
- 用途：Evolution 提案层；Pattern→Proposal（kind=ActionPattern），经 `validate` 校验（`confidence ≥ 0.6`（VALIDATE_THRESHOLD）→ Validated，否则 Rejected）。**Proposal 不改变 Agent 行为**。
- 主键：id
- 唯一约束：无（id 幂等 upsert，如 proposal.<action>）
- 索引：无

| 字段 | 类型 | 非空 | 默认 | 注释 | 枚举(若有) |
|------|------|------|------|------|------------|
| id | text | 是 | - | 提案ID（如 proposal.<action>） | - |
| kind | text | 是 | - | 提案类型 | ActionPattern（见 data-dict） |
| target | text | 是 | - | 目标动作名 | - |
| confidence | real | 是 | 0.0 | 置信度 | - |
| status | text | 是 | - | 提案状态 | Proposed / Validated / Rejected（见 data-dict） |
| pattern_label | text | 是 | - | 来源 Pattern（可追溯） | - |
| run_ids | text | 是 | '[]' | 贡献来源 run（JSON 数组） | - |
| created_at | text | 是 | CURRENT_TIMESTAMP | 创建时间 | - |
| updated_at | text | 是 | CURRENT_TIMESTAMP | 更新时间 | - |

### 4.9 表结构：wb_learned
- 用途：Evolution 最终知识层；**仅 Validated 的 Proposal 才沉淀为 LearnedKnowledge**。携带 `proposal_id` 可追溯。**Learn≠Permission：本表不写回 Policy、不修改 check_action、不自动绕过 Human Approval（WB2005）**。
- 主键：id
- 唯一约束：无（id 幂等 upsert，如 learned.<action>）
- 索引：无

| 字段 | 类型 | 非空 | 默认 | 注释 | 枚举(若有) |
|------|------|------|------|------|------------|
| id | text | 是 | - | 知识ID（如 learned.<action>） | - |
| kind | text | 是 | - | 知识类型 | ActionPattern（见 data-dict） |
| action | text | 是 | - | 关联动作名 | - |
| confidence | real | 是 | 0.0 | 置信度 | - |
| proposal_id | text | 是 | - | 来源 Proposal（可追溯） | - |
| run_ids | text | 是 | '[]' | 贡献来源 run（JSON 数组） | - |
| auto_approve_enabled | integer | 是 | 1 | REQ-018 自动放行资格（可撤销，默认开）；撤销即置 0，下次该 action 命中 5 道闸门时回退 Human Approval | 0/1 |
| created_at | text | 是 | CURRENT_TIMESTAMP | 创建时间 | - |

### 4.10 表结构：wb_approval_audit
- 用途：REQ-018 自动放行审计；每次自动放行（`decision=auto_approve`）或资格切换（`decision=auto_approve_enabled` / `revoke_disabled`）落库，可追溯至 `LearnedKnowledge` + `run_id`；支撑「可审计」铁律。
- 主键：id
- 唯一约束：无
- 索引：无

| 字段 | 类型 | 非空 | 默认 | 注释 | 枚举(若有) |
|------|------|------|------|------|------------|
| id | text | 是 | - | 审计行稳定 id（audit_{ts}_{seq}） | - |
| run_id | text | 否 | - | 触发自动放行的运行（可追溯） | - |
| action | text | 是 | - | 被自动放行的动作 | - |
| learned_id | text | 是 | - | 依据哪条 LearnedKnowledge | - |
| decision | text | 是 | - | 决策类型 | auto_approve / auto_approve_enabled / revoke_disabled |
| reason | text | 否 | - | 决策说明（如「5 道闸门全过」/「撤销自动放行资格」） | - |
| created_at | text | 是 | CURRENT_TIMESTAMP | 创建时间 | - |


## 5. 风险点 & 注意事项
- 遗漏 is_delete 逻辑删除会导致物理删除风险。
- 枚举值在代码与文档漂移会引发状态错乱。

## 6. 关联文档
- [../04_数据库设计/db-total-erd.md](../04_数据库设计/db-total-erd.md)
- [../04_数据库设计/data-dict.md](../04_数据库设计/data-dict.md)
- [../04_数据库设计/sql-script.md](../04_数据库设计/sql-script.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成 |
| V1.1 | lushaohui | 2026-08-11 | 表清单新增 wb_trace_event，补充 4.3 表结构详情 |
| V1.2 | lushaohui | 2026-08-11 | 新增 wb_ontology_object / wb_relation_edge 两张核心表（4.4/4.5）；4.3 类型对齐 SQLite + id 改 TEXT；id 统一字段规则改为「系统表 INTEGER / 领域表 TEXT」 |
| V1.3 | lushaohui | 2026-08-12 | 新增 Agent 自治四表：wb_memory（V0.5·REQ-016 记忆沉淀）、wb_pattern / wb_proposal / wb_learned（BL-003·REQ-017 Evolution Engine，Learn≠Permission）；表清单 4.1 补 4 行，新增 4.6~4.9 结构详情 |
| V1.4 | lushaohui | 2026-08-12 | REQ-018 Evolution 第二阶段 AutoApprove：wb_learned 加 `auto_approve_enabled`（integer 0/1，可撤销资格，默认 1）；新增 wb_approval_audit 审计表（4.10，decision=auto_approve/auto_approve_enabled/revoke_disabled，可追溯至 LearnedKnowledge+run） |

