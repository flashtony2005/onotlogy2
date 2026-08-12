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
    r#"-- 元数据：schema 版本（open 据此拦截旧库，避免 CREATE TABLE IF NOT EXISTS 不迁移的坑）
CREATE TABLE IF NOT EXISTS wb_meta (
  key    TEXT PRIMARY KEY,
  value  TEXT
);

-- 统一事件轨迹：world 生命周期事件 + cognitive 认知事件（见 agent-observation-design.md 4.3 / 4.7）
CREATE TABLE IF NOT EXISTS wb_ontology_object (
  id          TEXT PRIMARY KEY,            -- 语义ID（如 customer_001）
  create_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  update_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  is_delete   INTEGER NOT NULL DEFAULT 0,  -- 逻辑删除 0未删 1已删
  type        TEXT NOT NULL,               -- 对象类型名（对应 ObjectType.name）
  data        TEXT NOT NULL,               -- 业务载荷 JSON
  version     INTEGER NOT NULL DEFAULT 1   -- 乐观锁版本，update 自增
);

CREATE TABLE IF NOT EXISTS wb_relation_edge (
  id          TEXT PRIMARY KEY,
  create_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  update_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  is_delete   INTEGER NOT NULL DEFAULT 0,
  relation    TEXT NOT NULL,               -- 关系类型（如 owns/belongs_to）
  source      TEXT NOT NULL,               -- 源对象ID
  target      TEXT NOT NULL,               -- 目标对象ID
  data        TEXT                        -- 关系载荷 JSON
);

CREATE TABLE IF NOT EXISTS wb_trace_event (
  id          TEXT PRIMARY KEY,
  create_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  update_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  is_delete   INTEGER NOT NULL DEFAULT 0,
  kind        TEXT NOT NULL,               -- world | cognitive
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
CREATE INDEX IF NOT EXISTS idx_trace_run    ON wb_trace_event (run_id);
CREATE INDEX IF NOT EXISTS idx_trace_agent  ON wb_trace_event (agent_id);
CREATE INDEX IF NOT EXISTS idx_trace_type   ON wb_trace_event (event_type);
CREATE INDEX IF NOT EXISTS idx_trace_time   ON wb_trace_event (timestamp);
CREATE INDEX IF NOT EXISTS idx_create_time  ON wb_trace_event (create_time);

-- V0.5 Agent 自治：记忆沉淀（Memory = Event 的积累；BL-008）。
-- 经验按 label 去重并累加 hits，下一轮 run 经 recall 复用，形成 Action→Memory→下次 Plan 的闭环。
CREATE TABLE IF NOT EXISTS wb_memory (
  label      TEXT PRIMARY KEY,   -- 记忆标签（如 action.retain / #RetentionPattern）
  payload    TEXT NOT NULL,      -- 经验载荷 JSON
  hits       INTEGER NOT NULL DEFAULT 1,  -- 命中/复用次数，自学习沉淀指标
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_memory_hits ON wb_memory (hits);

-- Evolution Engine（BL-003 · REQ-017）：Experience → Pattern → Proposal → Validation → LearnedKnowledge
-- 全部带可追溯父引用（run_ids / pattern_label / proposal_id），保证 Learn≠Permission（不修改 Policy、不绕过审批）。
CREATE TABLE IF NOT EXISTS wb_pattern (
  label      TEXT PRIMARY KEY,    -- pattern.action.retain
  action     TEXT NOT NULL,       -- 关联动作名
  support    INTEGER NOT NULL DEFAULT 0,  -- 支持度 = 来源记忆 hits
  confidence REAL NOT NULL DEFAULT 0.0,   -- min(1, hits/5)
  run_ids    TEXT NOT NULL DEFAULT '[]',  -- 贡献来源 run（可追溯至 AgentRun）
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS wb_proposal (
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

CREATE TABLE IF NOT EXISTS wb_learned (
  id          TEXT PRIMARY KEY,    -- learned.<action>（幂等 upsert）
  kind        TEXT NOT NULL,
  action      TEXT NOT NULL,
  confidence  REAL NOT NULL DEFAULT 0.0,
  proposal_id TEXT NOT NULL,       -- 来源 Proposal（可追溯）
  run_ids     TEXT NOT NULL DEFAULT '[]',
  auto_approve_enabled INTEGER NOT NULL DEFAULT 1,  -- REQ-018：自动放行资格（可撤销，默认开）
  created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS wb_approval_audit (
  id          TEXT PRIMARY KEY,    -- 审计行稳定 id
  run_id      TEXT,                -- 触发自动放行的运行（可追溯）
  action      TEXT NOT NULL,       -- 被自动放行的动作
  learned_id  TEXT NOT NULL,       -- 依据哪条 LearnedKnowledge
  decision    TEXT NOT NULL,       -- auto_approve | revoke_disabled | fallback_human
  reason      TEXT,                -- 决策说明（如「5 道闸门全过」/「审计写库失败回退」）
  created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
"#
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
