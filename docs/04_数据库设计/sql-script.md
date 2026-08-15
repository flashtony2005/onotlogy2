# 建表 SQL 与初始化脚本
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-04-004 |
| 所属模块 | OpenAIP-OS-DB |
| 版本 | V1.6 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
存放所有 DDL 与初始化 DML 脚本，作为库结构变更的唯一定义来源。

## 2. 业务/技术背景
由 table-design.md 衍生。

## 3. 核心规则 & 约束（重点）
- [强制] 所有表结构变更必须走此脚本，禁止直接在线改库。
- [强制] 脚本按版本分段，可重复执行（幂等）或带版本标记。
- [建议] 初始化数据区分「环境无关基础数据」与「环境相关种子数据」。

## 4. 详细设计/内容
### 4.1 建表脚本（V1.5.0）
```sql
-- 系统账户（内部账号，数值自增主键）
CREATE TABLE wb_user_account (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,  -- 主键
  create_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,  -- 创建时间
  update_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,  -- 更新时间
  is_delete   INTEGER NOT NULL DEFAULT 0,  -- 逻辑删除 0未删 1已删
  username    TEXT NOT NULL  -- 用户名
);

CREATE INDEX idx_create_time ON wb_user_account (create_time);

-- 元数据：schema 版本（open 据此拦截旧库，避免 CREATE TABLE IF NOT EXISTS 不迁移的坑；V0.2 引入）
CREATE TABLE wb_meta (
  key    TEXT PRIMARY KEY,  -- 元数据键（如 schema_version）
  value  TEXT  -- 元数据值
);

-- 本体对象实例（Everything is Object：每类业务对象统一存此表，禁止按业务建独立表；V0.1 引入）
CREATE TABLE wb_ontology_object (
  id          TEXT PRIMARY KEY,  -- 语义对象ID（如 customer_001）
  create_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,  -- 创建时间
  update_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,  -- 更新时间
  is_delete   INTEGER NOT NULL DEFAULT 0,  -- 逻辑删除 0未删 1已删
  type        TEXT NOT NULL,  -- 对象类型（对应 ODL object 名，如 Customer）
  data        TEXT NOT NULL,  -- 对象数据 JSON（由 ObjectDef 驱动，禁止 match 硬编码）
  version     INTEGER NOT NULL DEFAULT 1  -- 乐观锁版本号，update 自增
);

CREATE INDEX idx_obj_type   ON wb_ontology_object (type);
CREATE INDEX idx_obj_create ON wb_ontology_object (create_time);

-- 关系边（世界连接，图查询基础；V0.2 引入）
CREATE TABLE wb_relation_edge (
  id          TEXT PRIMARY KEY,  -- 语义边ID（如 e_001）
  create_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,  -- 创建时间
  update_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,  -- 更新时间
  is_delete   INTEGER NOT NULL DEFAULT 0,  -- 逻辑删除 0未删 1已删
  source      TEXT NOT NULL,  -- 源对象ID（FK → wb_ontology_object.id）
  relation    TEXT NOT NULL,  -- 关系名（如 purchased / owns）
  target      TEXT NOT NULL,  -- 目标对象ID（FK → wb_ontology_object.id）
  data        TEXT  -- 关系附载 JSON（可选）
);

CREATE INDEX idx_edge_source ON wb_relation_edge (source);
CREATE INDEX idx_edge_target ON wb_relation_edge (target);
CREATE INDEX idx_edge_rel    ON wb_relation_edge (relation);

-- 统一事件轨迹：world 生命周期事件 + cognitive 认知事件（见 agent-observation-design.md 4.3 / 4.7）
CREATE TABLE wb_trace_event (
  id          TEXT PRIMARY KEY,  -- 语义轨迹ID（如 tr_abc123），与 API DTO.id 直接对应
  create_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,  -- 记录创建时间
  update_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,  -- 记录更新时间
  is_delete   INTEGER NOT NULL DEFAULT 0,  -- 逻辑删除 0未删 1已删
  kind        TEXT NOT NULL,  -- 事件大类：world | cognitive（枚举见 data-dict）
  run_id      TEXT,  -- 认知运行ID（kind=cognitive 时必填）
  agent_id    TEXT,  -- Agent ID（kind=cognitive 时必填）
  event_type  TEXT NOT NULL,  -- 事件类型（枚举见 data-dict）
  object_refs TEXT,  -- 绑定的 Ontology 对象引用，JSON 数组，可解释性根基；反查经 wb_ontology_object.id
  reasoning   TEXT,  -- 推理过程（kind=cognitive 专有）
  input       TEXT,  -- 输入载荷 JSON
  output      TEXT,  -- 输出载荷 JSON
  confidence  REAL,  -- 置信度 0~1（kind=cognitive 专有）
  timestamp   TEXT NOT NULL  -- 事件发生时间（业务时间）
);

CREATE INDEX idx_trace_run    ON wb_trace_event (run_id);
CREATE INDEX idx_trace_agent  ON wb_trace_event (agent_id);
CREATE INDEX idx_trace_type   ON wb_trace_event (event_type);
CREATE INDEX idx_trace_time   ON wb_trace_event (timestamp);
CREATE INDEX idx_create_time  ON wb_trace_event (create_time);

-- 记忆沉淀（Agent 自治基础；V0.5 · REQ-016）：经验按 label 去重累加 hits，下一轮 run 经 recall 复用
CREATE TABLE wb_memory (
  label      TEXT PRIMARY KEY,   -- 记忆标签（如 action.retain / #RetentionPattern）
  payload    TEXT NOT NULL,      -- 经验载荷 JSON
  hits       INTEGER NOT NULL DEFAULT 1,  -- 命中/复用次数，自学习沉淀指标
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_memory_hits ON wb_memory (hits);

-- Evolution Engine（BL-003 · REQ-017）：Experience → Pattern → Proposal → Validation → LearnedKnowledge
-- 全部带可追溯父引用（run_ids / pattern_label / proposal_id），保证 Learn≠Permission（不修改 Policy、不绕过审批）
CREATE TABLE wb_pattern (
  label      TEXT PRIMARY KEY,    -- pattern.action.retain
  action     TEXT NOT NULL,       -- 关联动作名
  support    INTEGER NOT NULL DEFAULT 0,  -- 支持度 = 来源记忆 hits
  confidence REAL NOT NULL DEFAULT 0.0,   -- min(1, hits/5)
  run_ids    TEXT NOT NULL DEFAULT '[]',  -- 贡献来源 run（可追溯至 AgentRun）
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE wb_proposal (
  id          TEXT PRIMARY KEY,    -- proposal.<action>（幂等 upsert）
  kind        TEXT NOT NULL,       -- ActionPattern
  target      TEXT NOT NULL,       -- 目标动作名
  confidence  REAL NOT NULL DEFAULT 0.0,
  status      TEXT NOT NULL,       -- Proposed / Validated / Rejected
  pattern_label TEXT NOT NULL,     -- 来源 Pattern（可追溯）
  run_ids     TEXT NOT NULL DEFAULT '[]',
  created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE wb_learned (
  id          TEXT PRIMARY KEY,    -- learned.<action>（幂等 upsert）
  kind        TEXT NOT NULL,
  action      TEXT NOT NULL,
  confidence  REAL NOT NULL DEFAULT 0.0,
  proposal_id TEXT NOT NULL,       -- 来源 Proposal（可追溯）
  run_ids     TEXT NOT NULL DEFAULT '[]',
  auto_approve_enabled INTEGER NOT NULL DEFAULT 1,  -- REQ-018 自动放行资格（可撤销，默认开）
  created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE wb_approval_audit (
  id          TEXT PRIMARY KEY,    -- 审计行稳定 id（audit_{ts}_{seq}）
  run_id      TEXT,                -- 触发自动放行的运行（可追溯）
  action      TEXT NOT NULL,       -- 被自动放行的动作
  learned_id  TEXT NOT NULL,       -- 依据哪条 LearnedKnowledge
  decision    TEXT NOT NULL,       -- auto_approve | auto_approve_enabled | revoke_disabled
  reason      TEXT,                -- 决策说明（如「5 道闸门全过」/「撤销自动放行资格」）
  created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Phase 11 Domain Agent Evolution（WB-DOC-03-V11.0）：Evaluation = Pure Function(Event Log)。
-- wb_evaluation 是 Event Log 的**派生结果**（可删表经 Reducer 重建），不是事实源。
CREATE TABLE wb_evaluation (
  id                 TEXT PRIMARY KEY,   -- eval.<run_id>（幂等 upsert）
  run_id             TEXT NOT NULL,
  task_id            TEXT NOT NULL,
  skill_version      TEXT,               -- 执行所用 Skill 版本（可追溯）
  correctness        REAL,
  completeness       REAL,
  efficiency         REAL,
  reliability        REAL,
  policy_compliance  REAL,
  output_quality     REAL,
  aggregate_score    REAL NOT NULL DEFAULT 0.0,  -- 派生视图：已出现维度均值
  failure_modes      TEXT NOT NULL DEFAULT '[]', -- worker.failed error 去重排序
  evidence           TEXT NOT NULL DEFAULT '{}', -- 每维度证据+置信度 JSON（11A 证据系统）
  evaluator_version  TEXT NOT NULL,
  created_at         TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_eval_run  ON wb_evaluation(run_id);
CREATE INDEX idx_eval_task ON wb_evaluation(task_id);

-- wb_mutation：Skill 结构化变更提案 + 配对 Benchmark 结果（失败保留，不删除）。
CREATE TABLE wb_mutation (
  id                 TEXT PRIMARY KEY,   -- mutation.<target>.<candidate_version>（幂等）
  target_asset       TEXT NOT NULL,
  source_version     TEXT NOT NULL,
  candidate_version  TEXT NOT NULL,
  root_cause         TEXT NOT NULL,
  evidence           TEXT NOT NULL DEFAULT '[]',  -- run_ids JSON
  delta              TEXT NOT NULL,               -- 结构化 Delta JSON（白名单操作）
  validation_result  TEXT,                        -- ok / 错误信息
  benchmark_result   TEXT,                        -- Benchmark 判定理由 JSON
  status             TEXT NOT NULL,               -- PROPOSED/VALIDATED/BENCHMARKING/EXPERIMENTAL/ACTIVE/REJECTED/ARCHIVED
  promotion_reason   TEXT,
  created_at         TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_mutation_status ON wb_mutation(status);
CREATE INDEX idx_mutation_target ON wb_mutation(target_asset);

-- Phase 11C：任务集（离线 Benchmark 的一等概念）。TaskDefinition 持久化——
-- 历史任务集 = 按 task_id 聚合的 wb_evaluation；未来重放执行（replay execution）的输入来源。
CREATE TABLE wb_task (
  task_id    TEXT PRIMARY KEY,
  definition TEXT NOT NULL,                -- TaskDefinition JSON（expected_relations/artifacts/time_budget）
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
```

### 4.2 初始化脚本
```sql
-- 环境无关基础数据
```


## 5. 风险点 & 注意事项
- 脚本不可幂等会在重复执行时失败，需评估。

## 6. 关联文档
- [../04_数据库设计/table-design.md](../04_数据库设计/table-design.md)
- [../08_部署运维/deploy-steps.md](../08_部署运维/deploy-steps.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成 |
| V1.1 | lushaohui | 2026-08-11 | 新增 wb_trace_event 建表 SQL（统一 TraceEvent 模型，kind 区分 world/cognitive） |
| V1.2 | lushaohui | 2026-08-11 | DDL 方言统一为 Turso/SQLite（INTEGER/TEXT/REAL + AUTOINCREMENT，列注释改 -- 行注释） |
| V1.3 | lushaohui | 2026-08-11 | 新增核心表 wb_ontology_object / wb_relation_edge（SQLite 方言，TEXT 语义ID）；wb_trace_event.id 改为 TEXT 与 DTO 对齐；补齐 idx_create_time 索引 |
| V1.4 | lushaohui | 2026-08-11 | codegen(canonical_schema) 与 LibSQLStorage 字段/索引对齐本权威 DDL：object 表列 type、edge 表 source/relation/target；索引补全 idx_obj_create / idx_edge_source / idx_edge_target / idx_edge_rel（与文档一致），移除代码侧多余的 idx_rel_* |
| V1.5 | lushaohui | 2026-08-12 | 新增 wb_meta 元数据表（schema_version），作为 open 旧 schema 拦截（WB2101）的唯一依据；canonical_schema 与文档 DDL 保持一致 |
| V1.6 | lushaohui | 2026-08-12 | 新增 wb_memory（V0.5·REQ-016 记忆沉淀）+ wb_pattern / wb_proposal / wb_learned（BL-003·REQ-017 Evolution Engine）建表 DDL，与 codegen canonical_schema 完全一致 |
| V1.7 | lushaohui | 2026-08-12 | REQ-018 Evolution 第二阶段 AutoApprove：wb_learned 加 `auto_approve_enabled INTEGER NOT NULL DEFAULT 1`；新增 wb_approval_audit 审计表（decision=auto_approve/auto_approve_enabled/revoke_disabled），与 codegen canonical_schema 完全一致 |
| V1.8 | lushaohui | 2026-08-15 | Phase 11 Domain Agent Evolution（WB-DOC-03-V11.0）：新增 wb_evaluation（Event Log 派生结果表，可重建）+ wb_mutation（提案 + Benchmark，失败保留）建表 DDL 与索引，与 codegen canonical_schema 完全一致 |
| V1.9 | lushaohui | 2026-08-15 | Phase 11 · 11A 证据系统：wb_evaluation 加 `evidence` 列（每维度 Score{evidence,confidence} JSON，Unknown ≠ Success），与 codegen canonical_schema 完全一致 |
| V2.0 | lushaohui | 2026-08-15 | Phase 11C 离线 Benchmark：新增 wb_task（TaskDefinition 持久化，历史任务集一等概念），与 codegen canonical_schema 完全一致 |

