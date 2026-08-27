//! Codegen —— IR → Rust struct / schema.sql / ontology.json（对应 REQ-003）
//!
//! 架构约束（module-split.md）：对象存储统一进 `wb_ontology_object`，**禁止为每类业务建独立表**。
//! 故 `schema.sql` 生成规范化核心三表 DDL（与 sql-script.md 同源），而非按类型分表。

use crate::ir::{ObjectType, OntologyIR};
use serde::Serialize;

/// Codegen 错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Codegen 错误 #{code} — {message}")]
pub struct CodegenError {
    pub code: u32,
    pub message: String,
}

impl CodegenError {
    pub fn new(code: u32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// 一份 IR 的代码生成产物。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodegenOutput {
    /// 每个对象类型生成一个 Rust struct + `<TYPE>_DEF` 常量。
    pub rust: String,
    /// 规范化核心表 DDL（wb_ontology_object / wb_relation_edge / wb_trace_event / wb_memory）。
    pub schema_sql: String,
    /// IR 的 JSON 序列化（ontology.json）。
    pub ontology_json: String,
}

/// 编译期对象类型定义（对应 architecture-total.md 的 ObjectDef 契约）。
/// 内部持序列化 JSON，运行时可经 serde 解析驱动，杜绝 `match object_type` 硬编码分支。
pub struct ObjectDef {
    pub name: &'static str,
    pub json: &'static str,
}

/// 把 IR 生成为可编译代码产物。
pub fn codegen(ir: &OntologyIR) -> Result<CodegenOutput, CodegenError> {
    let mut rust_parts: Vec<String> = Vec::new();
    for ot in &ir.object_types {
        rust_parts.push(gen_struct(ot)?);
    }
    let rust = rust_parts.join("\n\n");
    let schema_sql = canonical_schema();
    let ontology_json =
        serde_json::to_string_pretty(ir).map_err(|e| CodegenError::new(2001, e.to_string()))?;
    Ok(CodegenOutput {
        rust,
        schema_sql,
        ontology_json,
    })
}

/// 生成一个对象类型的 Rust struct + 类型定义常量。
fn gen_struct(ot: &ObjectType) -> Result<String, CodegenError> {
    let mut s = String::new();
    s.push_str("#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]\n");
    s.push_str(&format!("pub struct {} {{\n", ot.name));
    for p in &ot.properties {
        s.push_str(&format!("    pub {}: {},\n", p.name, p.ty.rust_type()));
    }
    s.push_str("}\n\n");

    // `<TYPE>_DEF`：该对象类型的编译期定义（ObjectDef），运行时可 serde 解析驱动
    // （约束 / Persistence 复用，杜绝 match object_type 硬编码分支）。
    let def_json = serde_json::to_string(ot).map_err(|e| CodegenError::new(2001, e.to_string()))?;
    s.push_str(&format!(
        "pub const {}_DEF: openaip_os::codegen::ObjectDef = openaip_os::codegen::ObjectDef {{\n    name: {:?},\n    json: r#\"{}\"#,\n}};\n",
        ot.name.to_uppercase(),
        ot.name,
        def_json
    ));
    Ok(s)
}

/// 规范化核心表 DDL（与 sql-script.md 一致）。存储统一，不按类型分表。
pub fn canonical_schema() -> String {
    r##"-- 元数据：schema 版本（open 据此拦截旧库，避免 CREATE TABLE IF NOT EXISTS 不迁移的坑）
CREATE TABLE IF NOT EXISTS wb_meta (
  key    TEXT PRIMARY KEY,
  value  TEXT
);

-- 迁移历史：version=1 是当前 schema 的基线，后续行记录每个已应用目标版本。
CREATE TABLE IF NOT EXISTS wb_schema_migration (
  version    INTEGER PRIMARY KEY,
  name       TEXT NOT NULL,
  applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_schema_migration_applied_at
  ON wb_schema_migration (applied_at);

-- 统一事件轨迹：world 生命周期事件 + cognitive 认知事件（见 agent-observation-design.md 4.3 / 4.7）
CREATE TABLE IF NOT EXISTS wb_ontology_object (
  id          TEXT PRIMARY KEY,            -- 语义ID（如 customer_001）
  create_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  update_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  is_delete   INTEGER NOT NULL DEFAULT 0,  -- 逻辑删除 0未删 1已删
  type        TEXT NOT NULL,               -- 对象类型名（对应 ObjectType.name）
  data        TEXT NOT NULL,               -- 业务载荷 JSON
  version     INTEGER NOT NULL DEFAULT 1,  -- 乐观锁版本，update 自增
  project_id  TEXT                         -- Phase 8：项目归属（NULL=全局）
);

CREATE TABLE IF NOT EXISTS wb_relation_edge (
  id          TEXT PRIMARY KEY,
  create_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  update_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  is_delete   INTEGER NOT NULL DEFAULT 0,
  relation    TEXT NOT NULL,               -- 关系类型（如 owns/belongs_to）
  source      TEXT NOT NULL,               -- 源对象ID
  target      TEXT NOT NULL,               -- 目标对象ID
  data        TEXT,                        -- 关系载荷 JSON
  project_id  TEXT                         -- Phase 8：项目归属（NULL=全局）
);

CREATE TABLE IF NOT EXISTS wb_trace_event (
  id          TEXT PRIMARY KEY,
  create_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  update_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  is_delete   INTEGER NOT NULL DEFAULT 0,
  kind        TEXT NOT NULL,               -- world | cognitive
  project_id  TEXT,                         -- Phase 8：项目归属（NULL=全局）
  run_id      TEXT,                         -- 认知运行ID（kind=cognitive 必填）
  agent_id    TEXT,                         -- Agent ID（kind=cognitive 必填）
  event_type  TEXT NOT NULL,                -- 事件类型
  object_refs TEXT,                         -- 绑定对象ID数组 JSON，可解释性根基
  reasoning   TEXT,                         -- 推理过程（kind=cognitive 专有）
  input       TEXT,                         -- 输入载荷 JSON
  output      TEXT,                         -- 输出载荷 JSON
  confidence  REAL,                         -- 置信度 0~1（kind=cognitive 专有）
  timestamp   TEXT NOT NULL                 -- 事件发生时间（业务时间）
);

CREATE INDEX IF NOT EXISTS idx_obj_type    ON wb_ontology_object (type);
CREATE INDEX IF NOT EXISTS idx_obj_create  ON wb_ontology_object (create_time);
CREATE INDEX IF NOT EXISTS idx_edge_source ON wb_relation_edge (source);
CREATE INDEX IF NOT EXISTS idx_edge_target ON wb_relation_edge (target);
CREATE INDEX IF NOT EXISTS idx_edge_rel    ON wb_relation_edge (relation);
-- 部分唯一索引：活跃边（is_delete=0）的 (source,relation,target) 不重复。
-- 杜绝 link() 并发 TOCTOU 双插（事务内 SELECT+INSERT 仅作旧库兜底，此索引为硬保证）。
CREATE UNIQUE INDEX IF NOT EXISTS uq_edge_active
  ON wb_relation_edge (source, relation, target) WHERE is_delete = 0;
CREATE INDEX IF NOT EXISTS idx_trace_run    ON wb_trace_event (run_id);
CREATE INDEX IF NOT EXISTS idx_trace_project ON wb_trace_event (project_id);
CREATE INDEX IF NOT EXISTS idx_trace_agent  ON wb_trace_event (agent_id);
CREATE INDEX IF NOT EXISTS idx_trace_type   ON wb_trace_event (event_type);
CREATE INDEX IF NOT EXISTS idx_trace_time   ON wb_trace_event (timestamp);
CREATE INDEX IF NOT EXISTS idx_create_time  ON wb_trace_event (create_time);

-- 审计轨迹是事实源：只能 INSERT，修正必须追加新事件，禁止 UPDATE / DELETE。
CREATE TRIGGER IF NOT EXISTS trg_wb_trace_event_append_only_update
BEFORE UPDATE ON wb_trace_event
BEGIN
  SELECT RAISE(ABORT, 'wb_trace_event is append-only');
END;
CREATE TRIGGER IF NOT EXISTS trg_wb_trace_event_append_only_delete
BEFORE DELETE ON wb_trace_event
BEGIN
  SELECT RAISE(ABORT, 'wb_trace_event is append-only');
END;

-- S2d：审计事件与完整性哈希链一一对应；链表同样只允许追加。
CREATE TABLE IF NOT EXISTS wb_audit_chain (
  sequence      INTEGER PRIMARY KEY,
  event_id      TEXT NOT NULL UNIQUE,
  previous_hash TEXT NOT NULL,
  hash          TEXT NOT NULL
);
CREATE TRIGGER IF NOT EXISTS trg_wb_audit_chain_append_only_update
BEFORE UPDATE ON wb_audit_chain
BEGIN
  SELECT RAISE(ABORT, 'wb_audit_chain is append-only');
END;
CREATE TRIGGER IF NOT EXISTS trg_wb_audit_chain_append_only_delete
BEFORE DELETE ON wb_audit_chain
BEGIN
  SELECT RAISE(ABORT, 'wb_audit_chain is append-only');
END;

-- V0.5 Agent 自治：记忆沉淀（Memory = Event 的积累；BL-008）。
-- 经验按 label 去重并累加 hits，下一轮 run 经 recall 复用，形成 Action→Memory→下次 Plan 的闭环。


-- Evolution Engine（BL-003 · REQ-017）：Experience → Pattern → Proposal → Validation → LearnedKnowledge
-- 全部带可追溯父引用（run_ids / pattern_label / proposal_id），保证 Learn≠Permission（不修改 Policy、不绕过审批）。




-- Phase 11 Domain Agent Evolution（WB-DOC-03-V11.0）：Evaluation = Pure Function(Event Log)。
-- wb_evaluation 是 Event Log 的**派生结果**（可删表经 Reducer 重建），不是事实源。
CREATE TABLE IF NOT EXISTS wb_evaluation (
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
  worker_outcomes    TEXT NOT NULL DEFAULT '[]', -- per-worker 执行结果（WorkerStats 可回放重建的数据源）
  evaluator_version  TEXT NOT NULL,
  created_at         TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_eval_run ON wb_evaluation(run_id);
CREATE INDEX IF NOT EXISTS idx_eval_task ON wb_evaluation(task_id);

-- wb_mutation：Skill 结构化变更提案 + 配对 Benchmark 结果（失败保留，不删除）。
CREATE TABLE IF NOT EXISTS wb_mutation (
  id                 TEXT PRIMARY KEY,   -- mutation.<target>.<candidate_version>（幂等）
  target_asset       TEXT NOT NULL,
  source_version     TEXT NOT NULL,
  candidate_version  TEXT NOT NULL,
  root_cause         TEXT NOT NULL,
  evidence           TEXT NOT NULL DEFAULT '[]',  -- run_ids JSON
  delta              TEXT NOT NULL,               -- 结构化 Delta JSON（白名单操作）
  validation_result  TEXT,                        -- ok / 错误信息
  benchmark_result   TEXT,                        -- BenchmarkVerdict JSON
  status             TEXT NOT NULL,               -- PROPOSED/VALIDATED/BENCHMARKING/EXPERIMENTAL/ACTIVE/REJECTED/ARCHIVED
  promotion_reason   TEXT,
  created_at         TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_mutation_status ON wb_mutation(status);
CREATE INDEX IF NOT EXISTS idx_mutation_target ON wb_mutation(target_asset);

-- Phase 11C：任务集（离线 Benchmark 的一等概念）。TaskDefinition 持久化——
-- 历史任务集 = 按 task_id 聚合的 wb_evaluation；未来重放执行（replay execution）的输入来源。
CREATE TABLE IF NOT EXISTS wb_task (
  task_id    TEXT PRIMARY KEY,
  definition TEXT NOT NULL,                -- TaskDefinition JSON（expected_relations/artifacts/time_budget）
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- V2 Worker Registry 持久化（S1 Decision Replay 的前置：重跑 Router 需要 declared 属性）。
-- manifest 全量：id/type/capabilities/评分维度/执行协议。
CREATE TABLE IF NOT EXISTS wb_worker (
  id           TEXT PRIMARY KEY,
  worker_type  TEXT NOT NULL,
  capabilities TEXT NOT NULL DEFAULT '[]',   -- JSON 数组
  quality      REAL NOT NULL DEFAULT 0.0,
  trust        REAL NOT NULL DEFAULT 0.0,
  risk         INTEGER NOT NULL DEFAULT 1,
  cost         INTEGER NOT NULL DEFAULT 1,
  latency_ms   INTEGER NOT NULL DEFAULT 100,
  availability TEXT NOT NULL DEFAULT 'local',
  execution    TEXT NOT NULL DEFAULT 'Process',
  created_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- V2 Router Selection Learning（Trust 轴）：(capability, worker) 执行统计。
-- 派生层（可回放重建）；只影响 Router 评分（Policy Filter 之后）；Capability/Approval 永不学习。
CREATE TABLE IF NOT EXISTS wb_worker_stats (
  capability       TEXT NOT NULL,
  worker_id        TEXT NOT NULL,
  runs             INTEGER NOT NULL DEFAULT 0,
  success          INTEGER NOT NULL DEFAULT 0,
  total_duration_ms INTEGER NOT NULL DEFAULT 0,
  created_at       TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at       TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (capability, worker_id)
);

-- Phase 2.5：业务包自动运行引擎任务队列。
-- 任务状态是宿主运行时事实，业务结果仍由插件写入本体对象。
CREATE TABLE IF NOT EXISTS wb_automation_job (
  id            TEXT PRIMARY KEY,
  pack_id       TEXT NOT NULL,
  job_type      TEXT NOT NULL,
  dedupe_key    TEXT NOT NULL,
  payload       TEXT NOT NULL,
  status        TEXT NOT NULL,
  attempt       INTEGER NOT NULL DEFAULT 0,
  max_attempts  INTEGER NOT NULL DEFAULT 3,
  available_at  TEXT NOT NULL,
  started_at    TEXT,
  finished_at   TEXT,
  last_error    TEXT,
  result        TEXT,
  created_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE (pack_id, job_type, dedupe_key)
);
CREATE INDEX IF NOT EXISTS idx_automation_job_status_available
  ON wb_automation_job (status, available_at, created_at);
-- 业务目标优化的活跃任务按项目唯一；数据库约束是多实例部署下的正确性边界。
CREATE UNIQUE INDEX IF NOT EXISTS uq_automation_active_business_optimization_project
  ON wb_automation_job (json_extract(payload, '$.projectId'))
  WHERE pack_id = 'business-loop'
    AND job_type = 'optimize'
    AND status IN ('queued', 'running');

-- V0.7 认证与授权（P0 风险「无认证」）：本地账号 + 不透明会话令牌。
-- 不依赖外键（libsql 默认 foreign_keys=OFF）；用户禁用时由应用层级联清理会话（delete_sessions_for_user）。
CREATE TABLE IF NOT EXISTS wb_user (
  id            TEXT PRIMARY KEY,
  username      TEXT NOT NULL UNIQUE,
  password_hash TEXT NOT NULL,             -- argon2id PHC 编码串
  role          TEXT NOT NULL,             -- executive | operator | developer
  display_name  TEXT NOT NULL DEFAULT '',
  disabled      INTEGER NOT NULL DEFAULT 0,-- 1=禁用（其会话立即失效）
  created_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_user_name ON wb_user(username);

CREATE TABLE IF NOT EXISTS wb_session (
  token      TEXT PRIMARY KEY,             -- 不透明随机令牌（hex 编码 32B）
  user_id    TEXT NOT NULL,                -- 逻辑关联 wb_user.id（无 FK，应用层维护）
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  expires_at TEXT NOT NULL                -- 过期时间（unix 秒，字符串数值比较）
);
CREATE INDEX IF NOT EXISTS idx_session_user ON wb_session(user_id);

-- V0.7 运行态持久化（P0 风险「运行态全在内存」）：run 生命周期 + 待审批请求持久化。
-- 内存 runs / ApprovalStore 仍服务热路径与 oneshot 唤醒；下表为持久真相源，支持跨重启恢复。
CREATE TABLE IF NOT EXISTS wb_run (
  id            TEXT PRIMARY KEY,        -- run_id
  agent_id      TEXT NOT NULL,
  goal          TEXT NOT NULL,
  status        TEXT NOT NULL,           -- Running | Completed | Failed | Interrupted
  human_approval INTEGER NOT NULL DEFAULT 0,
  project_id    TEXT,                      -- Phase 8：项目归属（NULL=全局）
  owner_user_id TEXT,                      -- authenticated principal owning the run
  created_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  finished_at   TEXT
);
CREATE INDEX IF NOT EXISTS idx_run_agent ON wb_run(agent_id);
CREATE INDEX IF NOT EXISTS idx_run_owner ON wb_run(owner_user_id);

-- Phase 8：项目注册表（项目 = 世界/事件/运营的顶层组织单元）。
CREATE TABLE IF NOT EXISTS wb_project (
  id          TEXT PRIMARY KEY,
  name        TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  ontology    TEXT NOT NULL DEFAULT '',
  knowledge_version_id TEXT REFERENCES wb_ontology_version(id),
  owner_user_id TEXT,
  status      TEXT NOT NULL DEFAULT 'active',
  created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_project_owner ON wb_project(owner_user_id);
CREATE INDEX IF NOT EXISTS idx_run_status ON wb_run(status);

-- 知识层：不可变本体版本。revision 是数据库分配的线性版本序。
CREATE TABLE IF NOT EXISTS wb_ontology_version (
  revision    INTEGER PRIMARY KEY AUTOINCREMENT,
  id          TEXT NOT NULL UNIQUE,
  parent_id   TEXT,
  ontology    TEXT NOT NULL,
  runtime_ir  TEXT,
  created_at  TEXT NOT NULL,
  FOREIGN KEY (parent_id) REFERENCES wb_ontology_version(id)
);
CREATE INDEX IF NOT EXISTS idx_ontology_version_parent
  ON wb_ontology_version (parent_id, revision);

-- 知识层：本体演化提案与人工评审审计记录。
CREATE TABLE IF NOT EXISTS wb_ontology_proposal (
  id                 TEXT PRIMARY KEY,
  source_version_id  TEXT NOT NULL REFERENCES wb_ontology_version(id),
  proposal_json      TEXT NOT NULL,
  review_json        TEXT NOT NULL,
  candidate_runtime_ir TEXT,
  status             TEXT NOT NULL,
  applied_version_id TEXT REFERENCES wb_ontology_version(id),
  created_at         TEXT NOT NULL,
  reviewed_at        TEXT
);
CREATE INDEX IF NOT EXISTS idx_ontology_proposal_status
  ON wb_ontology_proposal (status, created_at);

-- 企业共享资源账本：一条动作可包含多个资源维度；写入时用 IMMEDIATE 事务
-- 做限额仲裁，读取时按 ODL 声明的滚动窗口聚合。
CREATE TABLE IF NOT EXISTS wb_shared_resource_usage (
  id             TEXT NOT NULL,
  project_id     TEXT NOT NULL,
  resource_name  TEXT NOT NULL,
  amount         REAL NOT NULL,
  executed_at    TEXT NOT NULL,
  created_at     TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (id, resource_name)
);
CREATE INDEX IF NOT EXISTS idx_shared_resource_window
  ON wb_shared_resource_usage (resource_name, executed_at);

-- 业务执行租约：崩溃后区分“仍在执行”与“可恢复中断”，避免盲目重复外部副作用。
CREATE TABLE IF NOT EXISTS wb_business_execution_lease (
  id             TEXT PRIMARY KEY,
  project_id     TEXT NOT NULL,
  opportunity_id TEXT NOT NULL,
  owner          TEXT NOT NULL,
  status         TEXT NOT NULL,
  expires_at     TEXT NOT NULL,
  created_at     TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at     TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_business_lease_active
  ON wb_business_execution_lease (opportunity_id, status, expires_at);
CREATE INDEX IF NOT EXISTS idx_business_lease_expiry
  ON wb_business_execution_lease (status, expires_at);

-- 业务优化运行历史：目标、停止原因和完整 cycle 明细可跨请求回看。
CREATE TABLE IF NOT EXISTS wb_business_optimization_run (
  id            TEXT PRIMARY KEY,
  project_id    TEXT NOT NULL,
  goal_metric   TEXT,
  stopped_as    TEXT NOT NULL,
  payload       TEXT NOT NULL,
  created_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_business_optimization_project
  ON wb_business_optimization_run (project_id, created_at DESC);

"##
.to_string()
}

/// 便捷：直接序列化 IR 为 JSON（供 ontology.json 写盘）。
pub fn to_json(ir: &OntologyIR) -> Result<String, CodegenError> {
    serde_json::to_string_pretty(ir).map_err(|e| CodegenError::new(2001, e.to_string()))
}

/// 校验序列化往返一致性（TC-002 保真：无信息丢失）。
pub fn roundtrip_ok(ir: &OntologyIR) -> Result<bool, CodegenError> {
    let json = to_json(ir)?;
    let back: OntologyIR =
        serde_json::from_str(&json).map_err(|e| CodegenError::new(2001, e.to_string()))?;
    Ok(back == *ir)
}

// 让 Serialize 特征在测试中可被引用（避免未使用告警）。
#[allow(dead_code)]
fn _assert_serialize<T: Serialize>() {}
