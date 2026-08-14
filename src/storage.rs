//! 存储层 —— ObjectEngine 抽象 + LibSQLStorage（Turso 落地）（对应 REQ-004 / REQ-005）
//!
//! 对象统一存 `wb_ontology_object`（语义ID）；TraceEvent 统一存 `wb_trace_event`。
//! 使用 libSQL 0.9 异步 API。列名与 sql-script.md 权威 DDL 一致（type / source / relation / target）。
//! 另用 `wb_meta.schema_version` 记录 schema 版本，open 据此拦截旧库（WB2101）。

use crate::codegen;
use crate::runtime::TraceEvent;
use async_trait::async_trait;
use libsql::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};

/// 关系边自增序列（用于语义ID）。
static REL_SEQ: AtomicU64 = AtomicU64::new(0);

/// 审计行自增序列（REQ-018 自动放行审计 id）。
static TRACE_SEQ_AUDIT: AtomicU64 = AtomicU64::new(0);

/// REQ-018：一条自动放行审计记录（可追溯至 LearnedKnowledge + run）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ApprovalAudit {
    pub id: String,
    pub run_id: String,
    pub action: String,
    pub learned_id: String,
    /// `auto_approve` | `revoke_disabled` | `fallback_human`
    pub decision: String,
    pub reason: String,
    pub created_at: String,
}

/// 本地账号（V0.7 认证）。
pub struct UserRecord {
    pub id: String,
    pub username: String,
    pub password_hash: String,
    pub role: String,
    pub display_name: String,
    pub disabled: bool,
}

/// 会话行（V0.7 认证）：不透明令牌 + 过期时间。
pub struct SessionRecord {
    pub token: String,
    pub user_id: String,
    pub created_at: String,
    pub expires_at: String,
}

fn now_ts() -> String {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", d.as_micros())
}

/// 一条对象记录（对应 `wb_ontology_object`）。`r#type` 映射 DDL 列 `type`（Rust 关键字需裸标识）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObjectRecord {
    pub id: String,
    pub r#type: String,
    pub data: Value,
    pub version: i64,
}

/// 一条关系记录（对应 `wb_relation_edge`）。字段名与列名一致：source / relation / target。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RelationRecord {
    pub id: String,
    pub source: String,
    pub relation: String,
    pub target: String,
    pub data: Option<Value>,
}

/// 一条记忆记录（对应 `wb_memory`，V0.5 记忆沉淀 / BL-008）。
/// 经验按 `label` 去重，重复沉淀累加 `hits`，作为自学习指标。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryRecord {
    pub label: String,
    pub payload: Value,
    pub hits: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// 存储错误。
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("数据库错误: {0}")]
    Db(#[from] libsql::Error),
    #[error("对象不存在: {0}")]
    NotFound(String),
    #[error("序列化错误: {0}")]
    Json(#[from] serde_json::Error),
    #[error("约束违反 {code}: {message}")]
    Constraint { code: &'static str, message: String },
}

/// Trace 事件查询条件（run/agent/object/type 过滤 + 游标 after_id 断线重连补齐）。
/// 列名取自固定白名单（run_id/agent_id/event_type/object_refs），值全部参数化绑定，防注入。
#[derive(Debug, Clone, Default)]
pub struct TraceQuery {
    pub run_id: Option<String>,
    pub agent_id: Option<String>,
    pub object_ref: Option<String>,
    pub event_type: Option<String>,
    pub after_id: Option<String>,
    pub limit: usize,
}

impl TraceQuery {
    /// 按 run 过滤（默认上限 200）。
    pub fn for_run(run_id: impl Into<String>) -> Self {
        Self {
            run_id: Some(run_id.into()),
            limit: 200,
            ..Default::default()
        }
    }
    /// 按 agent 过滤。
    pub fn for_agent(agent_id: impl Into<String>) -> Self {
        Self {
            agent_id: Some(agent_id.into()),
            limit: 200,
            ..Default::default()
        }
    }
    /// 设置断线重连游标：仅返回 after_id 之后的事件。
    pub fn with_after(mut self, after_id: impl Into<String>) -> Self {
        self.after_id = Some(after_id.into());
        self
    }
    /// 按绑定对象过滤。
    pub fn with_object(mut self, object_ref: impl Into<String>) -> Self {
        self.object_ref = Some(object_ref.into());
        self
    }
}

/// 对象引擎抽象（V0.1 仅同步语义，async 由实现决定）。
#[async_trait]
pub trait ObjectEngine {
    async fn create(
        &self,
        object_type: &str,
        id: &str,
        data: Value,
    ) -> Result<ObjectRecord, StorageError>;
    async fn get(&self, id: &str) -> Result<Option<ObjectRecord>, StorageError>;
    async fn update(&self, id: &str, data: Value) -> Result<ObjectRecord, StorageError>;
    async fn query(&self, object_type: &str) -> Result<Vec<ObjectRecord>, StorageError>;
    async fn delete(&self, id: &str) -> Result<(), StorageError>;
}

/// 关系引擎抽象（V0.2 World Model 要素二）。
#[async_trait]
pub trait RelationEngine {
    /// 建边；相同 (source, relation, target) 已存在则拒 WB2007。
    async fn link(
        &self,
        source_id: &str,
        edge_type: &str,
        target_id: &str,
        data: Option<Value>,
    ) -> Result<RelationRecord, StorageError>;
    /// 从某对象出发的所有出边。
    async fn neighbors(&self, source_id: &str) -> Result<Vec<RelationRecord>, StorageError>;
    /// 按边类型查全部边。
    async fn query_by_edge(&self, edge_type: &str) -> Result<Vec<RelationRecord>, StorageError>;
    /// 逻辑删除一条边。
    async fn unlink(
        &self,
        source_id: &str,
        edge_type: &str,
        target_id: &str,
    ) -> Result<(), StorageError>;
}

/// 基于 libSQL（Turso）的实现。持有单连接以共享同一 in-memory / 文件库。
/// `Connection` 在 libsql 中为廉价克隆（内部 Arc），故整体可 `Clone`，
/// 便于把同一存储句柄分给认证/会话等长期存活的提供方（V0.7）。
#[derive(Clone)]
pub struct LibSQLStorage {
    conn: Connection,
}

/// 当前 schema 版本。每次 schema 变更（加列 / 改列名）递增；旧库 open 时据此拦截（WB2101）。
const SCHEMA_VERSION: &str = "1";

/// 判断 libsql 错误是否为 `no such table`（跨创建方稳定，优于 PRAGMA / sqlite_master 文本探测）。
fn is_no_such_table(e: &libsql::Error) -> bool {
    e.to_string().contains("no such table")
}

impl LibSQLStorage {
    /// 打开（或创建）数据库并初始化核心表，含旧 schema 拦截（WB2101）。
    pub async fn open(path: &str) -> Result<Self, StorageError> {
        let db = libsql::Builder::new_local(path).build().await?;
        let conn = db.connect()?;
        Self::ensure_schema(&conn).await?;
        Ok(Self { conn })
    }

    /// 初始化连接：先判定新 / 旧库，再建 / 刷 schema。
    ///
    /// 判定策略（规避 libsql 下 PRAGMA / sqlite_master 文本探测跨创建方不稳定的坑）：
    /// 1. `SELECT id FROM wb_ontology_object LIMIT 0` 探针判断对象表是否存在
    ///    （`id` 在新旧 schema 均存在，故只回答“表在不在”，不依赖具体列名）。
    /// 2. 表不存在 → 全新库：建全套核心 schema + 写入 `wb_meta.schema_version`。
    /// 3. 表存在 → 读 `wb_meta.schema_version`：
    ///    - 无 `wb_meta` / 无版本行 / 版本不符 → 旧 schema，返 WB2101 引导 `oaip db reset`。
    ///    - 版本匹配 → 幂等刷新 schema（未来增量列在此落地）。
    async fn ensure_schema(conn: &Connection) -> Result<(), StorageError> {
        let has_obj = match conn
            .query("SELECT id FROM wb_ontology_object LIMIT 0", params![])
            .await
        {
            Ok(_) => true,
            Err(e) if is_no_such_table(&e) => false,
            Err(e) => return Err(StorageError::Db(e)),
        };

        if !has_obj {
            // 全新库：建全套核心表（含 wb_meta），写入版本
            conn.execute_batch(&codegen::canonical_schema()).await?;
            conn.execute(
                "INSERT OR REPLACE INTO wb_meta (key, value) VALUES ('schema_version', ?1)",
                params![SCHEMA_VERSION],
            )
            .await?;
            return Ok(());
        }

        // 已有对象表 → 读版本（无 wb_meta / 版本不符 → 旧 schema）
        let version = match conn
            .query(
                "SELECT value FROM wb_meta WHERE key='schema_version'",
                params![],
            )
            .await
        {
            Ok(mut rows) => match rows.next().await? {
                Some(r) => r.get::<String>(0).unwrap_or_default(),
                None => String::new(),
            },
            Err(e) if is_no_such_table(&e) => String::new(),
            Err(e) => return Err(StorageError::Db(e)),
        };

        if version != SCHEMA_VERSION {
            return Err(StorageError::Constraint {
                code: "WB2101",
                message:
                    "检测到旧 schema 或版本不匹配（列 object_type/from_id/to_id/edge_type）。请运行 `oaip db reset` 重建数据库"
                        .into(),
            });
        }

        // 版本匹配 → 幂等刷新 schema
        conn.execute_batch(&codegen::canonical_schema()).await?;
        // REQ-018 迁移：为已有 wb_learned 补 auto_approve_enabled 列（新库已由 canonical_schema 包含）；
        // 列已存在则 ALTER 报错，忽略即可（幂等）。
        let _ = conn
            .execute(
                "ALTER TABLE wb_learned ADD COLUMN auto_approve_enabled INTEGER NOT NULL DEFAULT 1",
                params![],
            )
            .await;
        Ok(())
    }

    /// 从已有连接构造（测试可用）。
    pub fn from_connection(conn: Connection) -> Self {
        Self { conn }
    }

    /// 落一条 TraceEvent 到 `wb_trace_event`。
    /// [强制] object_refs 为空 → 拦截返回 WB2006，不落库（对应 TC-007-b）。
    pub async fn append_trace(&self, ev: &TraceEvent) -> Result<(), StorageError> {
        if ev.object_refs.is_empty() {
            return Err(StorageError::Constraint {
                code: "WB2006",
                message: "TraceEvent 缺 object_refs，禁止落库".into(),
            });
        }
        let obj_refs = serde_json::to_string(&ev.object_refs)?;
        let input = ev.input.as_ref().map(serde_json::to_string).transpose()?;
        let output = ev.output.as_ref().map(serde_json::to_string).transpose()?;
        self.conn
            .execute(
                "INSERT INTO wb_trace_event \
                 (id, kind, event_type, object_refs, run_id, agent_id, reasoning, input, output, confidence, timestamp) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                params![
                    ev.id.clone(),
                    ev.kind.as_str(),
                    ev.event_type.clone(),
                    obj_refs,
                    ev.run_id.clone(),
                    ev.agent_id.clone(),
                    ev.reasoning.clone(),
                    input,
                    output,
                    ev.confidence,
                    ev.timestamp.clone()
                ],
            )
            .await?;
        Ok(())
    }

    /// 按条件查询 Trace 事件（认知/世界同源）。
    /// [强制] after_id 游标用于 WebSocket 断线重连补齐：返回该事件之后的全部匹配事件（按 timestamp,id 升序）。
    pub async fn list_events(&self, q: TraceQuery) -> Result<Vec<TraceEvent>, StorageError> {
        let mut sql = String::from(
            "SELECT id, kind, event_type, object_refs, run_id, agent_id, reasoning, input, output, confidence, timestamp \
             FROM wb_trace_event WHERE 1=1",
        );
        let mut params: Vec<libsql::Value> = Vec::new();

        if let Some(v) = &q.run_id {
            sql.push_str(" AND run_id = ?");
            params.push(libsql::Value::Text(v.clone()));
        }
        if let Some(v) = &q.agent_id {
            sql.push_str(" AND agent_id = ?");
            params.push(libsql::Value::Text(v.clone()));
        }
        if let Some(v) = &q.event_type {
            sql.push_str(" AND event_type = ?");
            params.push(libsql::Value::Text(v.clone()));
        }
        if let Some(v) = &q.object_ref {
            // object_refs 存 JSON 数组文本，按 `"id"` 子串匹配（反查经 wb_ontology_object.id）。
            sql.push_str(" AND object_refs LIKE ?");
            params.push(libsql::Value::Text(format!("%\"{}\"%", v)));
        }
        if let Some(after) = &q.after_id {
            // 解析游标事件 (timestamp, id)，返回其后事件（断线重连补齐）。
            let mut marker = self
                .conn
                .query(
                    "SELECT timestamp, id FROM wb_trace_event WHERE id = ?",
                    libsql::params![after.clone()],
                )
                .await?;
            if let Some(row) = marker.next().await? {
                let ts: String = row.get(0)?;
                let id: String = row.get(1)?;
                sql.push_str(" AND (timestamp > ? OR (timestamp = ? AND id > ?))");
                params.push(libsql::Value::Text(ts.clone()));
                params.push(libsql::Value::Text(ts));
                params.push(libsql::Value::Text(id));
            }
        }
        sql.push_str(" ORDER BY timestamp ASC, id ASC");
        sql.push_str(&format!(" LIMIT {}", q.limit.clamp(1, 1000)));

        let mut rows = self
            .conn
            .query(&sql, libsql::params_from_iter(params))
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_trace(&row)?);
        }
        Ok(out)
    }

    /// 按 ID 取单条 Trace 事件（供 `GET /trace/{id}`，缺失返 None → WB2002）。
    pub async fn get_trace(&self, id: &str) -> Result<Option<TraceEvent>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, kind, event_type, object_refs, run_id, agent_id, reasoning, input, output, confidence, timestamp \
                 FROM wb_trace_event WHERE id = ?",
                params![id],
            )
            .await?;
        if let Some(row) = rows.next().await? {
            Ok(Some(row_to_trace(&row)?))
        } else {
            Ok(None)
        }
    }

    /// 列出 world 事件（按时间升序），供 `oaip event list` 与 TC-008 校验。
    pub async fn list_world_events(&self) -> Result<Vec<TraceEvent>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, kind, event_type, object_refs, run_id, agent_id, reasoning, input, output, confidence, timestamp \
                 FROM wb_trace_event WHERE kind='world' ORDER BY timestamp ASC",
                params![],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_trace(&row)?);
        }
        Ok(out)
    }

    /// 列出全部对象（任意类型，is_delete=0），供世界模型快照。
    pub async fn list_all_objects(&self) -> Result<Vec<ObjectRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, type, data, version FROM wb_ontology_object WHERE is_delete=0",
                params![],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_object(&row)?);
        }
        Ok(out)
    }

    /// 列出全部关系边（is_delete=0），供世界模型快照。
    pub async fn list_all_relations(&self) -> Result<Vec<RelationRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, relation, source, target, data FROM wb_relation_edge WHERE is_delete=0",
                params![],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_relation(&row)?);
        }
        Ok(out)
    }

    /// 渲染对象列表。客户：`id name level`（TC-006）；任务：`id title status`。
    pub fn render_object_list(records: &[ObjectRecord]) -> String {
        let mut lines = Vec::new();
        for r in records {
            if r.data.get("status").is_some() {
                let title = r.data.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let status = r.data.get("status").and_then(|v| v.as_str()).unwrap_or("");
                lines.push(format!("{} {} {}", r.id, title, status));
            } else {
                let name = r.data.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let level = r.data.get("level").and_then(|v| v.as_str()).unwrap_or("");
                lines.push(format!("{} {} {}", r.id, name, level));
            }
        }
        lines.join("\n")
    }

    // ---- V0.5 Agent 自治：记忆沉淀（Memory = Event 积累；BL-008） ----

    /// 沉淀一条记忆：按 `label` upsert；已存在则累加 `hits` 并更新 `payload`，实现经验复用计数。
    pub async fn record_memory(
        &self,
        label: &str,
        payload: Value,
    ) -> Result<MemoryRecord, StorageError> {
        let payload_s = serde_json::to_string(&payload)?;
        self.conn
            .execute(
                "INSERT INTO wb_memory (label, payload, hits, created_at, updated_at) \
                 VALUES (?1, ?2, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP) \
                 ON CONFLICT(label) DO UPDATE SET \
                   hits = hits + 1, payload = ?2, updated_at = CURRENT_TIMESTAMP",
                params![label, payload_s],
            )
            .await?;
        self.recall_memory(label)
            .await?
            .ok_or_else(|| StorageError::NotFound(format!("memory:{label}")))
    }

    /// 按标签召回单条记忆（缺失 → None）。
    pub async fn recall_memory(&self, label: &str) -> Result<Option<MemoryRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT label, payload, hits, created_at, updated_at FROM wb_memory WHERE label = ?1",
                params![label],
            )
            .await?;
        match rows.next().await? {
            Some(row) => Ok(Some(row_to_memory(&row)?)),
            None => Ok(None),
        }
    }

    /// 召回全部记忆（按 hits 降序，复用频次高的在前）。
    pub async fn recall_all(&self) -> Result<Vec<MemoryRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT label, payload, hits, created_at, updated_at FROM wb_memory \
                 ORDER BY hits DESC, updated_at DESC",
                params![],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_memory(&row)?);
        }
        Ok(out)
    }

    // ---- Evolution Engine（BL-003 · REQ-017）：Experience → Knowledge，不修改 Policy ----

    /// 写入/合并一条 Pattern（按 label 幂等 upsert；run_ids 合并去重，沉淀跨 run 证据）。
    pub async fn save_pattern(
        &self,
        pattern: &crate::evolution::Pattern,
        run_ids: &[String],
    ) -> Result<(), StorageError> {
        // 读取已有 run_ids 以合并（证据跨 run 累加）
        let existing: Vec<String> = match self.read_pattern_run_ids(&pattern.label).await? {
            Some(s) => crate::evolution::parse_run_ids(&s),
            None => Vec::new(),
        };
        let merged = crate::evolution::merge_run_ids(existing, run_ids);
        let run_ids_s = crate::evolution::encode_run_ids(&merged);
        self.conn
            .execute(
                "INSERT INTO wb_pattern (label, action, support, confidence, run_ids, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP) \
                 ON CONFLICT(label) DO UPDATE SET \
                   support = ?3, confidence = ?4, run_ids = ?5, updated_at = CURRENT_TIMESTAMP",
                params![
                    pattern.label.as_str(),
                    pattern.action.as_str(),
                    pattern.support,
                    pattern.confidence,
                    run_ids_s
                ],
            )
            .await?;
        Ok(())
    }

    async fn read_pattern_run_ids(&self, label: &str) -> Result<Option<String>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT run_ids FROM wb_pattern WHERE label = ?1",
                params![label],
            )
            .await?;
        match rows.next().await? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    async fn read_proposal_run_ids(&self, id: &str) -> Result<Option<String>, StorageError> {
        let mut rows = self
            .conn
            .query("SELECT run_ids FROM wb_proposal WHERE id = ?1", params![id])
            .await?;
        match rows.next().await? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    async fn read_learned_run_ids(&self, id: &str) -> Result<Option<String>, StorageError> {
        let mut rows = self
            .conn
            .query("SELECT run_ids FROM wb_learned WHERE id = ?1", params![id])
            .await?;
        match rows.next().await? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    /// 列出全部 Pattern（按 support 降序）。
    pub async fn list_patterns(&self) -> Result<Vec<crate::evolution::Pattern>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT label, action, support, confidence, run_ids FROM wb_pattern \
                 ORDER BY support DESC, updated_at DESC",
                params![],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_pattern(&row)?);
        }
        Ok(out)
    }

    /// 写入一条 Proposal（按 id 幂等 upsert）。
    pub async fn save_proposal(
        &self,
        proposal: &crate::evolution::Proposal,
    ) -> Result<(), StorageError> {
        let status = match proposal.status {
            crate::evolution::ProposalStatus::Proposed => "Proposed",
            crate::evolution::ProposalStatus::Validated => "Validated",
            crate::evolution::ProposalStatus::Rejected => "Rejected",
        };
        let kind = match proposal.kind {
            crate::evolution::ProposalKind::ActionPattern => "ActionPattern",
        };
        // 合并已有 run_ids（证据跨 run 累加）
        let existing = self
            .read_proposal_run_ids(&proposal.id)
            .await?
            .map(|s| crate::evolution::parse_run_ids(&s))
            .unwrap_or_default();
        let merged = crate::evolution::merge_run_ids(existing, &proposal.run_ids);
        let run_ids_s = crate::evolution::encode_run_ids(&merged);
        self.conn
            .execute(
                "INSERT INTO wb_proposal (id, kind, target, confidence, status, pattern_label, run_ids, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP) \
                 ON CONFLICT(id) DO UPDATE SET \
                   confidence = ?4, status = ?5, run_ids = ?7, updated_at = CURRENT_TIMESTAMP",
                params![
                    proposal.id.as_str(),
                    kind,
                    proposal.target.as_str(),
                    proposal.confidence,
                    status,
                    proposal.pattern_label.as_str(),
                    run_ids_s
                ],
            )
            .await?;
        Ok(())
    }

    /// 列出全部 Proposal（按置信度降序）。
    pub async fn list_proposals(&self) -> Result<Vec<crate::evolution::Proposal>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, kind, target, confidence, status, pattern_label, run_ids FROM wb_proposal \
                 ORDER BY confidence DESC, updated_at DESC",
                params![],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_proposal(&row)?);
        }
        Ok(out)
    }

    /// 写入一条 LearnedKnowledge（按 id 幂等 upsert）。仅在 Proposal Validated 后调用。
    pub async fn save_learned(
        &self,
        learned: &crate::evolution::LearnedKnowledge,
    ) -> Result<(), StorageError> {
        let kind = match learned.kind {
            crate::evolution::ProposalKind::ActionPattern => "ActionPattern",
        };
        // 合并已有 run_ids（证据跨 run 累加）
        let existing = self
            .read_learned_run_ids(&learned.id)
            .await?
            .map(|s| crate::evolution::parse_run_ids(&s))
            .unwrap_or_default();
        let merged = crate::evolution::merge_run_ids(existing, &learned.run_ids);
        let run_ids_s = crate::evolution::encode_run_ids(&merged);
        self.conn
            .execute(
                "INSERT INTO wb_learned (id, kind, action, confidence, proposal_id, run_ids, auto_approve_enabled, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, CURRENT_TIMESTAMP) \
                 ON CONFLICT(id) DO UPDATE SET \
                   confidence = ?4, proposal_id = ?5, run_ids = ?6, auto_approve_enabled = ?7",
                params![
                    learned.id.as_str(),
                    kind,
                    learned.action.as_str(),
                    learned.confidence,
                    learned.proposal_id.as_str(),
                    run_ids_s,
                    learned.auto_approve_enabled as i64,
                ],
            )
            .await?;
        Ok(())
    }

    /// 列出全部 LearnedKnowledge（按置信度降序）。
    pub async fn list_learned(
        &self,
    ) -> Result<Vec<crate::evolution::LearnedKnowledge>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, kind, action, confidence, proposal_id, run_ids, auto_approve_enabled, created_at FROM wb_learned \
                 ORDER BY confidence DESC, created_at DESC",
                params![],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_learned(&row)?);
        }
        Ok(out)
    }

    /// 撤销/删除一条 LearnedKnowledge（幂等：不存在也返回 Ok）。
    pub async fn delete_learned(&self, id: &str) -> Result<(), StorageError> {
        self.conn
            .execute("DELETE FROM wb_learned WHERE id = ?1", params![id])
            .await?;
        Ok(())
    }

    /// REQ-018：翻转一条 LearnedKnowledge 的自动放行资格（可撤销承诺，幂等）。
    /// 置 false 后该 action 即使满足其余 4 道闸门也回退 Human Approval（决不静默放行）。
    pub async fn set_learned_auto_approve(
        &self,
        id: &str,
        enabled: bool,
    ) -> Result<(), StorageError> {
        self.conn
            .execute(
                "UPDATE wb_learned SET auto_approve_enabled = ?2 WHERE id = ?1",
                params![id, enabled as i64],
            )
            .await?;
        Ok(())
    }

    /// REQ-018：写入一条自动放行审计（可追溯至 LearnedKnowledge + run）。
    pub async fn record_approval_audit(
        &self,
        run_id: &str,
        action: &str,
        learned_id: &str,
        decision: &str,
        reason: &str,
    ) -> Result<(), StorageError> {
        let id = format!(
            "audit_{}_{}",
            now_ts(),
            TRACE_SEQ_AUDIT.fetch_add(1, Ordering::SeqCst)
        );
        self.conn
            .execute(
                "INSERT INTO wb_approval_audit (id, run_id, action, learned_id, decision, reason) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, run_id, action, learned_id, decision, reason],
            )
            .await?;
        Ok(())
    }

    /// REQ-018：列出自动放行审计（按 id 过滤单条 LearnedKnowledge，或全量）。
    pub async fn list_approval_audit(
        &self,
        learned_id: Option<&str>,
    ) -> Result<Vec<ApprovalAudit>, StorageError> {
        let mut rows = match learned_id {
            Some(lid) => {
                self.conn
                    .query(
                        "SELECT id, run_id, action, learned_id, decision, reason, created_at \
                         FROM wb_approval_audit WHERE learned_id = ?1 \
                         ORDER BY created_at DESC, id DESC",
                        params![lid],
                    )
                    .await?
            }
            None => {
                self.conn
                    .query(
                        "SELECT id, run_id, action, learned_id, decision, reason, created_at \
                         FROM wb_approval_audit ORDER BY created_at DESC, id DESC",
                        params![],
                    )
                    .await?
            }
        };
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(ApprovalAudit {
                id: row.get(0)?,
                run_id: row.get(1)?,
                action: row.get(2)?,
                learned_id: row.get(3)?,
                decision: row.get(4)?,
                reason: row.get(5)?,
                created_at: row.get(6)?,
            });
        }
        Ok(out)
    }

    /// REQ-018：统计某动作的历史 ActionFailed 事件数（强判定「无失败」闸门）。
    /// ActionFailed 事件的 `input` 存 `{"action":"<动作>"}`，据此精确匹配（避免误伤同名）。
    pub async fn count_action_failed(&self, action: &str) -> Result<usize, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT COUNT(*) FROM wb_trace_event \
                 WHERE event_type = 'ActionFailed' AND input LIKE ?1",
                params![format!("%\"action\":\"{action}\"%")],
            )
            .await?;
        let cnt: i64 = match rows.next().await? {
            Some(row) => row.get(0)?,
            None => 0,
        };
        Ok(cnt.max(0) as usize)
    }
}

fn row_to_pattern(row: &libsql::Row) -> Result<crate::evolution::Pattern, StorageError> {
    let run_ids: String = row.get(4)?;
    Ok(crate::evolution::Pattern {
        label: row.get(0)?,
        action: row.get(1)?,
        support: row.get(2)?,
        confidence: row.get(3)?,
        run_ids: crate::evolution::parse_run_ids(&run_ids),
    })
}

fn row_to_proposal(row: &libsql::Row) -> Result<crate::evolution::Proposal, StorageError> {
    let id: String = row.get(0)?;
    let kind_s: String = row.get(1)?;
    let target: String = row.get(2)?;
    let confidence: f64 = row.get(3)?;
    let status_s: String = row.get(4)?;
    let pattern_label: String = row.get(5)?;
    let run_ids: String = row.get(6)?;
    let kind = match kind_s.as_str() {
        "ActionPattern" => crate::evolution::ProposalKind::ActionPattern,
        _ => crate::evolution::ProposalKind::ActionPattern,
    };
    let status = match status_s.as_str() {
        "Validated" => crate::evolution::ProposalStatus::Validated,
        "Rejected" => crate::evolution::ProposalStatus::Rejected,
        _ => crate::evolution::ProposalStatus::Proposed,
    };
    Ok(crate::evolution::Proposal {
        id,
        kind,
        target,
        confidence,
        status,
        pattern_label,
        run_ids: crate::evolution::parse_run_ids(&run_ids),
    })
}

fn row_to_learned(row: &libsql::Row) -> Result<crate::evolution::LearnedKnowledge, StorageError> {
    let id: String = row.get(0)?;
    let kind_s: String = row.get(1)?;
    let action: String = row.get(2)?;
    let confidence: f64 = row.get(3)?;
    let proposal_id: String = row.get(4)?;
    let run_ids: String = row.get(5)?;
    // wb_learned.auto_approve_enabled 为 INTEGER（0/1），libsql 不自动转 bool，需手动映射。
    let auto_approve_enabled: bool = row.get::<i64>(6)? != 0;
    let created_at: String = row.get(7)?;
    let kind = match kind_s.as_str() {
        "ActionPattern" => crate::evolution::ProposalKind::ActionPattern,
        _ => crate::evolution::ProposalKind::ActionPattern,
    };
    Ok(crate::evolution::LearnedKnowledge {
        id,
        kind,
        action,
        confidence,
        proposal_id,
        run_ids: crate::evolution::parse_run_ids(&run_ids),
        auto_approve_enabled,
        created_at,
    })
}

#[async_trait]
impl ObjectEngine for LibSQLStorage {
    async fn create(
        &self,
        object_type: &str,
        id: &str,
        data: Value,
    ) -> Result<ObjectRecord, StorageError> {
        let data_str = serde_json::to_string(&data)?;
        self.conn
            .execute(
                "INSERT INTO wb_ontology_object (id, type, data, version) VALUES (?1,?2,?3,1)",
                params![id, object_type, data_str],
            )
            .await?;
        Ok(ObjectRecord {
            id: id.to_string(),
            r#type: object_type.to_string(),
            data,
            version: 1,
        })
    }

    async fn get(&self, id: &str) -> Result<Option<ObjectRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, type, data, version FROM wb_ontology_object WHERE id=?1 AND is_delete=0",
                params![id],
            )
            .await?;
        if let Some(row) = rows.next().await? {
            Ok(Some(row_to_object(&row)?))
        } else {
            Ok(None)
        }
    }

    async fn update(&self, id: &str, data: Value) -> Result<ObjectRecord, StorageError> {
        let data_str = serde_json::to_string(&data)?;
        let mut rows = self
            .conn
            .query(
                "SELECT type, version FROM wb_ontology_object WHERE id=?1 AND is_delete=0",
                params![id],
            )
            .await?;
        let (object_type, version) = if let Some(row) = rows.next().await? {
            (row.get::<String>(0)?, row.get::<i64>(1)?)
        } else {
            return Err(StorageError::NotFound(id.to_string()));
        };
        let new_version = version + 1;
        self.conn
            .execute(
                "UPDATE wb_ontology_object SET data=?1, version=?2, update_time=CURRENT_TIMESTAMP WHERE id=?3",
                params![data_str, new_version, id],
            )
            .await?;
        Ok(ObjectRecord {
            id: id.to_string(),
            r#type: object_type,
            data,
            version: new_version,
        })
    }

    async fn query(&self, object_type: &str) -> Result<Vec<ObjectRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, type, data, version FROM wb_ontology_object WHERE type=?1 AND is_delete=0",
                params![object_type],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_object(&row)?);
        }
        Ok(out)
    }

    async fn delete(&self, id: &str) -> Result<(), StorageError> {
        let n = self
            .conn
            .execute(
                "UPDATE wb_ontology_object SET is_delete=1, update_time=CURRENT_TIMESTAMP WHERE id=?1",
                params![id],
            )
            .await?;
        if n == 0 {
            return Err(StorageError::NotFound(id.to_string()));
        }
        Ok(())
    }
}

impl LibSQLStorage {
    /// 硬删除（物理删行）。仅供演示/CLI 重灌，产品路径用 `ObjectEngine::delete` 软删除。
    pub async fn hard_delete(&self, id: &str) -> Result<u64, StorageError> {
        let n = self
            .conn
            .execute("DELETE FROM wb_ontology_object WHERE id=?1", params![id])
            .await?;
        Ok(n)
    }
}

// ── V0.7 运行态持久化（P0 风险「运行态全在内存」）：wb_run / wb_approval ──

/// 一条 run 持久记录（与内存 `AppState.runs` 同义，作为跨重启真相源）。
pub struct RunRecord {
    pub id: String,
    pub agent_id: String,
    pub goal: String,
    pub status: String, // Running | Completed | Failed | Interrupted
    pub human_approval: bool,
    pub created_at: String,
    pub updated_at: String,
    pub finished_at: Option<String>,
}

/// 一条待审批持久记录（与内存 `ApprovalStore` 同义，支持跨重启列表/审计）。
pub struct ApprovalRecord {
    pub id: String,
    pub run_id: String,
    pub agent_id: String,
    pub step_id: String,
    pub description: String,
    pub state: String, // Pending | Approved | Rejected | Expired
    pub created_at: String,
    pub updated_at: String,
    pub resolved_by: Option<String>,
    pub resolved_at: Option<String>,
}

impl LibSQLStorage {
    /// 插入一条 run（status='Running'，默认 human_approval=0）。热路径与内存映射并行写。
    pub async fn insert_run(
        &self,
        id: &str,
        agent_id: &str,
        goal: &str,
    ) -> Result<(), StorageError> {
        self.conn
            .execute(
                "INSERT INTO wb_run (id, agent_id, goal, status) VALUES (?1,?2,?3,'Running')",
                params![id, agent_id, goal],
            )
            .await?;
        Ok(())
    }

    /// 更新 run 终态（Completed/Failed/Interrupted）+ 完成时间（None → 不填）。
    pub async fn update_run_status(
        &self,
        id: &str,
        status: &str,
        finished_at: Option<&str>,
    ) -> Result<(), StorageError> {
        let finished: libsql::Value = match finished_at {
            Some(s) => libsql::Value::Text(s.to_string()),
            None => libsql::Value::Null,
        };
        self.conn
            .execute(
                "UPDATE wb_run SET status=?1, finished_at=?2, updated_at=CURRENT_TIMESTAMP WHERE id=?3",
                libsql::params![status, finished, id],
            )
            .await?;
        Ok(())
    }

    /// 列出全部 run（恢复重建内存映射用）。
    pub async fn list_runs(&self) -> Result<Vec<RunRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, agent_id, goal, status, human_approval, created_at, updated_at, finished_at \
                 FROM wb_run ORDER BY created_at ASC",
                params![],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().await? {
            out.push(row_to_run(&r)?);
        }
        Ok(out)
    }

    /// 按 id 取单条 run（timeline DB 回退用）。
    pub async fn get_run(&self, id: &str) -> Result<Option<RunRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, agent_id, goal, status, human_approval, created_at, updated_at, finished_at \
                 FROM wb_run WHERE id = ?1",
                params![id],
            )
            .await?;
        match rows.next().await? {
            Some(r) => Ok(Some(row_to_run(&r)?)),
            None => Ok(None),
        }
    }

    /// 插入一条待审批（status='Pending'）。
    pub async fn insert_approval(
        &self,
        req_id: &str,
        run_id: &str,
        agent_id: &str,
        step_id: &str,
        description: &str,
    ) -> Result<(), StorageError> {
        self.conn
            .execute(
                "INSERT INTO wb_approval (id, run_id, agent_id, step_id, description, state) \
                 VALUES (?1,?2,?3,?4,?5,'Pending')",
                params![req_id, run_id, agent_id, step_id, description],
            )
            .await?;
        Ok(())
    }

    /// 更新待审批裁决状态（resolved_by/resolved_at 为 None 时不填）。
    pub async fn update_approval_state(
        &self,
        id: &str,
        state: &str,
        resolved_by: Option<&str>,
        resolved_at: Option<&str>,
    ) -> Result<(), StorageError> {
        let by: libsql::Value = match resolved_by {
            Some(s) => libsql::Value::Text(s.to_string()),
            None => libsql::Value::Null,
        };
        let at: libsql::Value = match resolved_at {
            Some(s) => libsql::Value::Text(s.to_string()),
            None => libsql::Value::Null,
        };
        self.conn
            .execute(
                "UPDATE wb_approval SET state=?1, resolved_by=?2, resolved_at=?3, updated_at=CURRENT_TIMESTAMP WHERE id=?4",
                libsql::params![state, by, at, id],
            )
            .await?;
        Ok(())
    }

    /// 列出某 run 的全部待审批（跨重启列表/审计）。
    pub async fn list_approvals_for_run(
        &self,
        run_id: &str,
    ) -> Result<Vec<ApprovalRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, run_id, agent_id, step_id, description, state, created_at, updated_at, resolved_by, resolved_at \
                 FROM wb_approval WHERE run_id = ?1 ORDER BY created_at ASC",
                params![run_id],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().await? {
            out.push(row_to_approval(&r)?);
        }
        Ok(out)
    }

    /// 按状态列出待审批（恢复时取 Pending）。
    pub async fn list_approvals_by_state(
        &self,
        state: &str,
    ) -> Result<Vec<ApprovalRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, run_id, agent_id, step_id, description, state, created_at, updated_at, resolved_by, resolved_at \
                 FROM wb_approval WHERE state = ?1 ORDER BY created_at ASC",
                params![state],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().await? {
            out.push(row_to_approval(&r)?);
        }
        Ok(out)
    }

    /// 重启恢复（简化分支）：所有 Running → Interrupted，所有 Pending → Expired；
    /// 返回全部 run 供重建内存映射。无"活 run"可恢复，故不做"run 存活→Pending 保留"死分支。
    pub async fn recover_persisted_state(&self) -> Result<Vec<RunRecord>, StorageError> {
        self.conn
            .execute(
                "UPDATE wb_run SET status='Interrupted', updated_at=CURRENT_TIMESTAMP WHERE status='Running'",
                params![],
            )
            .await?;
        self.conn
            .execute(
                "UPDATE wb_approval SET state='Expired', updated_at=CURRENT_TIMESTAMP WHERE state='Pending'",
                params![],
            )
            .await?;
        self.list_runs().await
    }
}

fn row_to_run(row: &libsql::Row) -> Result<RunRecord, StorageError> {
    let finished_at: Option<String> = row.get(7)?;
    Ok(RunRecord {
        id: row.get(0)?,
        agent_id: row.get(1)?,
        goal: row.get(2)?,
        status: row.get(3)?,
        human_approval: row.get::<i64>(4)? != 0,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        finished_at,
    })
}

fn row_to_approval(row: &libsql::Row) -> Result<ApprovalRecord, StorageError> {
    let resolved_by: Option<String> = row.get(8)?;
    let resolved_at: Option<String> = row.get(9)?;
    Ok(ApprovalRecord {
        id: row.get(0)?,
        run_id: row.get(1)?,
        agent_id: row.get(2)?,
        step_id: row.get(3)?,
        description: row.get(4)?,
        state: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
        resolved_by,
        resolved_at,
    })
}

#[async_trait]
impl RelationEngine for LibSQLStorage {
    async fn link(
        &self,
        source_id: &str,
        edge_type: &str,
        target_id: &str,
        data: Option<Value>,
    ) -> Result<RelationRecord, StorageError> {
        // 重复边检测（相同 source/relation/target 且未删）→ 拒 WB2007
        let mut dup = self
            .conn
            .query(
                "SELECT id FROM wb_relation_edge WHERE source=?1 AND relation=?2 AND target=?3 AND is_delete=0",
                params![source_id, edge_type, target_id],
            )
            .await?;
        if dup.next().await?.is_some() {
            return Err(StorageError::Constraint {
                code: "WB2007",
                message: format!("重复关系边 {source_id}-{edge_type}->{target_id}"),
            });
        }
        let id = format!(
            "rel_{}_{}",
            now_ts(),
            REL_SEQ.fetch_add(1, Ordering::SeqCst)
        );
        let data_str = data.as_ref().map(serde_json::to_string).transpose()?;
        self.conn
            .execute(
                "INSERT INTO wb_relation_edge (id, relation, source, target, data) VALUES (?1,?2,?3,?4,?5)",
                params![id.clone(), edge_type, source_id, target_id, data_str],
            )
            .await?;
        Ok(RelationRecord {
            id,
            source: source_id.to_string(),
            relation: edge_type.to_string(),
            target: target_id.to_string(),
            data,
        })
    }

    async fn neighbors(&self, source_id: &str) -> Result<Vec<RelationRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, relation, source, target, data FROM wb_relation_edge WHERE source=?1 AND is_delete=0",
                params![source_id],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_relation(&row)?);
        }
        Ok(out)
    }

    async fn query_by_edge(&self, edge_type: &str) -> Result<Vec<RelationRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, relation, source, target, data FROM wb_relation_edge WHERE relation=?1 AND is_delete=0",
                params![edge_type],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_relation(&row)?);
        }
        Ok(out)
    }

    async fn unlink(
        &self,
        source_id: &str,
        edge_type: &str,
        target_id: &str,
    ) -> Result<(), StorageError> {
        self.conn
            .execute(
                "UPDATE wb_relation_edge SET is_delete=1, update_time=CURRENT_TIMESTAMP \
                 WHERE source=?1 AND relation=?2 AND target=?3 AND is_delete=0",
                params![source_id, edge_type, target_id],
            )
            .await?;
        Ok(())
    }
}

fn row_to_object(row: &libsql::Row) -> Result<ObjectRecord, StorageError> {
    let data: String = row.get(2)?;
    Ok(ObjectRecord {
        id: row.get(0)?,
        r#type: row.get(1)?,
        data: serde_json::from_str(&data)?,
        version: row.get(3)?,
    })
}

fn row_to_relation(row: &libsql::Row) -> Result<RelationRecord, StorageError> {
    let data: Option<String> = row.get(4)?;
    Ok(RelationRecord {
        id: row.get(0)?,
        relation: row.get(1)?,
        source: row.get(2)?,
        target: row.get(3)?,
        data: data.map(|s| serde_json::from_str(&s)).transpose()?,
    })
}

fn row_to_memory(row: &libsql::Row) -> Result<MemoryRecord, StorageError> {
    let payload: String = row.get(1)?;
    Ok(MemoryRecord {
        label: row.get(0)?,
        payload: serde_json::from_str(&payload)?,
        hits: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
    })
}

fn row_to_trace(row: &libsql::Row) -> Result<TraceEvent, StorageError> {
    let obj_refs: String = row.get(3)?;
    let run_id: Option<String> = row.get(4)?;
    let agent_id: Option<String> = row.get(5)?;
    let reasoning: Option<String> = row.get(6)?;
    let input: Option<String> = row.get(7)?;
    let output: Option<String> = row.get(8)?;
    let confidence: Option<f64> = row.get(9)?;
    let kind_str: String = row.get(1)?;
    Ok(TraceEvent {
        id: row.get(0)?,
        kind: crate::runtime::TraceKind::from_str(&kind_str)
            .unwrap_or(crate::runtime::TraceKind::World),
        event_type: row.get(2)?,
        object_refs: serde_json::from_str(&obj_refs)?,
        run_id,
        agent_id,
        reasoning,
        input: input.map(|s| serde_json::from_str(&s)).transpose()?,
        output: output.map(|s| serde_json::from_str(&s)).transpose()?,
        confidence,
        timestamp: row.get(10)?,
    })
}

// ── 认证与授权（V0.7 · 对应 P0 风险「无认证」）──
impl LibSQLStorage {
    /// 创建本地账号（用户名唯一约束由 wb_user.username UNIQUE 保证）。
    pub async fn create_user(&self, u: &UserRecord) -> Result<(), StorageError> {
        self.conn
            .execute(
                "INSERT INTO wb_user (id, username, password_hash, role, display_name, disabled) VALUES (?1,?2,?3,?4,?5,?6)",
                params![
                    u.id.clone(),
                    u.username.clone(),
                    u.password_hash.clone(),
                    u.role.clone(),
                    u.display_name.clone(),
                    u.disabled as i64
                ],
            )
            .await?;
        Ok(())
    }

    /// 按用户名取账号（登录校验）；不存在返回 None。
    pub async fn get_user_by_username(
        &self,
        username: &str,
    ) -> Result<Option<UserRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, username, password_hash, role, display_name, disabled FROM wb_user WHERE username = ?1",
                params![username],
            )
            .await?;
        match rows.next().await? {
            Some(r) => Ok(Some(user_from_row(&r)?)),
            None => Ok(None),
        }
    }

    /// 账号总数（启动引导判定：空库且未配 env 则拒绝启动）。
    pub async fn count_users(&self) -> Result<i64, StorageError> {
        let mut rows = self
            .conn
            .query("SELECT COUNT(*) FROM wb_user", params![])
            .await?;
        match rows.next().await? {
            Some(r) => Ok(r.get(0)?),
            None => Ok(0),
        }
    }

    /// 按 id 取账号（会话校验反查）；不存在返回 None。
    pub async fn get_user_by_id(&self, id: &str) -> Result<Option<UserRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, username, password_hash, role, display_name, disabled FROM wb_user WHERE id = ?1",
                params![id],
            )
            .await?;
        match rows.next().await? {
            Some(r) => Ok(Some(user_from_row(&r)?)),
            None => Ok(None),
        }
    }

    /// 列出全部账号。
    pub async fn list_users(&self) -> Result<Vec<UserRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, username, password_hash, role, display_name, disabled FROM wb_user",
                params![],
            )
            .await?;
        let mut users = Vec::new();
        while let Some(r) = rows.next().await? {
            users.push(user_from_row(&r)?);
        }
        Ok(users)
    }

    /// 返回对象-关系图（GraphRAG 基础）：节点来自对象，边来自关系。
    pub async fn get_object_graph(
        &self,
    ) -> Result<(Vec<ObjectRecord>, Vec<RelationRecord>), StorageError> {
        Ok((self.list_all_objects().await?, self.list_all_relations().await?))
    }

    /// 删除账号（幂等：不存在则 NotFound）。
    pub async fn delete_user(&self, username: &str) -> Result<(), StorageError> {
        let Some(u) = self.get_user_by_username(username).await? else {
            return Err(StorageError::NotFound(format!("user:{username}")));
        };
        self.conn
            .execute("DELETE FROM wb_user WHERE username = ?1", params![username])
            .await?;
        self.delete_sessions_for_user(&u.id).await?;
        Ok(())
    }

    /// 启停账号（禁用时级联清其全部会话，应用层替代外键 ON DELETE CASCADE）。
    pub async fn set_user_disabled(
        &self,
        user_id: &str,
        disabled: bool,
    ) -> Result<(), StorageError> {
        self.conn
            .execute(
                "UPDATE wb_user SET disabled = ?2 WHERE id = ?1",
                params![user_id, disabled as i64],
            )
            .await?;
        if disabled {
            self.delete_sessions_for_user(user_id).await?;
        }
        Ok(())
    }

    /// 签发会话行（令牌由调用方生成，见 `SessionStore`）。
    pub async fn save_session(
        &self,
        token: &str,
        user_id: &str,
        expires_at: &str,
    ) -> Result<(), StorageError> {
        self.conn
            .execute(
                "INSERT INTO wb_session (token, user_id, expires_at) VALUES (?1,?2,?3)",
                params![token, user_id, expires_at],
            )
            .await?;
        Ok(())
    }

    /// 取会话行（含 user_id，用于反查用户与过期判定）。
    pub async fn get_session(&self, token: &str) -> Result<Option<SessionRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT token, user_id, created_at, expires_at FROM wb_session WHERE token = ?1",
                params![token],
            )
            .await?;
        match rows.next().await? {
            Some(r) => Ok(Some(SessionRecord {
                token: r.get(0)?,
                user_id: r.get(1)?,
                created_at: r.get(2)?,
                expires_at: r.get(3)?,
            })),
            None => Ok(None),
        }
    }

    /// 撤销单条会话（登出 / 禁用级联）。
    pub async fn delete_session(&self, token: &str) -> Result<(), StorageError> {
        self.conn
            .execute("DELETE FROM wb_session WHERE token = ?1", params![token])
            .await?;
        Ok(())
    }

    /// 撤销某用户全部会话（禁用账号时级联）。
    pub async fn delete_sessions_for_user(&self, user_id: &str) -> Result<(), StorageError> {
        self.conn
            .execute(
                "DELETE FROM wb_session WHERE user_id = ?1",
                params![user_id],
            )
            .await?;
        Ok(())
    }
}

/// 从 wb_user 行构造 `UserRecord`。
fn user_from_row(r: &libsql::Row) -> Result<UserRecord, StorageError> {
    Ok(UserRecord {
        id: r.get(0)?,
        username: r.get(1)?,
        password_hash: r.get(2)?,
        role: r.get(3)?,
        display_name: r.get(4)?,
        disabled: r.get::<i64>(5)? != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // 旧 schema（有对象表但无 wb_meta / 版本）→ ensure_schema 拒绝 WB2101
    #[tokio::test]
    async fn ensure_schema_rejects_legacy() {
        let db = libsql::Builder::new_local(":memory:")
            .build()
            .await
            .unwrap();
        let conn = db.connect().unwrap();
        conn.execute_batch(
            "CREATE TABLE wb_ontology_object (id TEXT PRIMARY KEY, object_type TEXT, data TEXT);\
             CREATE TABLE wb_relation_edge (id TEXT PRIMARY KEY, edge_type TEXT, from_id TEXT, to_id TEXT);",
        )
        .await
        .unwrap();
        let err = LibSQLStorage::ensure_schema(&conn)
            .await
            .expect_err("应返回 WB2101");
        match err {
            StorageError::Constraint { code, .. } => assert_eq!(code, "WB2101"),
            other => panic!("旧 schema 应返回 WB2101，实际: {other:?}"),
        }
    }

    // 全新库 → open 成功建表并写入 schema 版本
    #[tokio::test]
    async fn open_creates_fresh_db_with_version() {
        let path = std::env::temp_dir().join(format!("oaip_fresh_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let storage = LibSQLStorage::open(path.to_str().unwrap()).await.unwrap();
        storage
            .create("Customer", "x1", serde_json::json!({"name": "A"}))
            .await
            .unwrap();
        let _ = std::fs::remove_file(&path);
    }
}
