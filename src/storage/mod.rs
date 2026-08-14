//! 存储层 —— ObjectEngine 抽象 + LibSQLStorage（Turso 落地）（对应 REQ-004 / REQ-005）
//!
//! 对象统一存 `wb_ontology_object`（语义ID）；TraceEvent 统一存 `wb_trace_event`。
//! 使用 libSQL 0.9 异步 API。列名与 sql-script.md 权威 DDL 一致（type / source / relation / target）。
//! 另用 `wb_meta.schema_version` 记录 schema 版本，open 据此拦截旧库（WB2101）。
//!
//! 模块拆分（长文件模块化）：核心类型/初始化在 `mod`，对象在 `objects`，关系在 `relations`，
//! 事件在 `events`，记忆在 `memory`，演化在 `evolution`，运行态在 `runs`，账号会话在 `users`。

pub mod events;
pub mod evolution;
pub mod memory;
pub mod objects;
pub mod relations;
pub mod runs;
pub mod users;

use crate::codegen;
pub(crate) use crate::runtime::TraceEvent;
use async_trait::async_trait;
use libsql::Connection;
use libsql::params;
pub use runs::{ApprovalRecord, RunRecord};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};

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

/// 当前 schema 版本。每次 schema 变更（加列 / 改列名）递增；
/// 旧库 open 时按 `MIGRATIONS` 顺序自动迁移（事务化），不再要求 `db reset`。
const SCHEMA_VERSION: i64 = 2;

/// 版本化迁移注册表：`(from_version, sql)`，按版本升序执行；每步成功后推进
/// `wb_meta.schema_version`。迁移在事务内执行（失败整体回滚，版本不推进，可安全重试）。
/// 新库由 `canonical_schema` 直接建到当前版本，不执行迁移。
const MIGRATIONS: &[(i64, &str)] = &[
    // V1 → V2：wb_learned 补 auto_approve_enabled（REQ-018）。
    // 此前该 ALTER 硬编码在「版本匹配」分支且吞错；现为正式迁移项。
    (
        1,
        "ALTER TABLE wb_learned ADD COLUMN auto_approve_enabled INTEGER NOT NULL DEFAULT 1",
    ),
];

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
    ///    - 无 `wb_meta` / 无版本行 → 旧 schema，返 WB2101 引导 `oaip db reset`。
    ///    - 版本 < 当前 → 按 `MIGRATIONS` 顺序事务化迁移（每步更新版本，失败整体回滚）。
    ///    - 版本 > 当前 → 返 WB2101（未来版本库，拒绝打开）。
    ///    - 版本匹配 → 幂等刷新 schema。
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
                params![SCHEMA_VERSION.to_string()],
            )
            .await?;
            return Ok(());
        }

        // 已有对象表 → 读版本（无 wb_meta / 无版本行 → 旧 schema）
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
        let cur: i64 = match version.parse() {
            Ok(v) => v,
            Err(_) => {
                return Err(StorageError::Constraint {
                    code: "WB2101",
                    message: "检测到无法识别的 schema 版本（无 wb_meta / 无版本行）。请运行 `oaip db reset` 重建数据库"
                        .into(),
                });
            }
        };
        if cur > SCHEMA_VERSION {
            return Err(StorageError::Constraint {
                code: "WB2101",
                message: format!(
                    "数据库 schema 版本 {cur} 高于程序支持的 {SCHEMA_VERSION}（请升级程序后再打开）"
                ),
            });
        }

        // C1 版本化迁移：按 MIGRATIONS 升序执行，事务保证原子（失败整体回滚，版本不推进）。
        if cur < SCHEMA_VERSION {
            let tx = conn.transaction().await?;
            let mut v = cur;
            for (from, sql) in MIGRATIONS {
                if *from >= v {
                    tx.execute_batch(sql).await?;
                    v = from + 1;
                    tx.execute(
                        "INSERT OR REPLACE INTO wb_meta (key, value) VALUES ('schema_version', ?1)",
                        params![v.to_string()],
                    )
                    .await?;
                }
            }
            tx.commit().await?;
        }

        // 版本匹配 → 幂等刷新 schema
        conn.execute_batch(&codegen::canonical_schema()).await?;
        Ok(())
    }

    /// 从已有连接构造（测试可用）。
    pub fn from_connection(conn: Connection) -> Self {
        Self { conn }
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

/// 转义 LIKE 通配符（`%`/`_`/`\`），配合 `ESCAPE '\'` 使用（M6：防通配符放大匹配）。
fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

// ── 认证与授权（V0.7 · 对应 P0 风险「无认证」）──

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

    // 并发 update 同一对象：version 原子递增，不得丢更新（读-改-写竞态回归）
    #[tokio::test]
    async fn concurrent_updates_increment_version_atomically() {
        let s = LibSQLStorage::open(":memory:").await.unwrap();
        s.create(
            "Customer",
            "001",
            serde_json::json!({"name": "A", "level": "VIP"}),
        )
        .await
        .unwrap();
        let s1 = s.clone();
        let s2 = s.clone();
        let (a, b) = tokio::join!(
            async {
                s1.update("001", serde_json::json!({"name": "A", "level": "GOLD"}))
                    .await
                    .unwrap()
            },
            async {
                s2.update("001", serde_json::json!({"name": "A", "level": "PLATINUM"}))
                    .await
                    .unwrap()
            },
        );
        // 两次更新都生效：version 1 → 2 → 3（返回值是 {2,3} 的某个排列）
        let mut versions = [a.version, b.version];
        versions.sort();
        assert_eq!(versions, [2, 3], "并发 update 不得丢版本（原子递增）");
        let rec = s.get("001").await.unwrap().unwrap();
        assert_eq!(rec.version, 3, "最终版本应为 3（两次更新均生效）");
    }

    // 并发 link 同一对边：事务串行化，恰一条成功、另一条 WB2007，不得双插（TOCTOU 回归）
    #[tokio::test]
    async fn concurrent_link_same_edge_no_duplicate() {
        let s = LibSQLStorage::open(":memory:").await.unwrap();
        s.create("Customer", "001", serde_json::json!({"name": "A"}))
            .await
            .unwrap();
        s.create("Task", "t1", serde_json::json!({"title": "T"}))
            .await
            .unwrap();
        let s1 = s.clone();
        let s2 = s.clone();
        let (r1, r2) = tokio::join!(async { s1.link("001", "owns", "t1", None).await }, async {
            s2.link("001", "owns", "t1", None).await
        },);
        let ok_count = [r1.is_ok(), r2.is_ok()].iter().filter(|b| **b).count();
        assert_eq!(ok_count, 1, "并发 link 应恰一条成功");
        let dup = [r1, r2].into_iter().find_map(|r| match r {
            Err(StorageError::Constraint { code, .. }) => Some(code.to_string()),
            _ => None,
        });
        assert_eq!(dup.as_deref(), Some("WB2007"), "另一条应拒 WB2007");
        let edges = s.neighbors("001").await.unwrap();
        assert_eq!(edges.len(), 1, "不得双插同一条边");
    }

    // C1 迁移：V1 旧库（wb_learned 缺 auto_approve_enabled 列）→ open 自动迁移到当前版本
    #[tokio::test]
    async fn open_migrates_v1_schema_to_current() {
        let path = std::env::temp_dir().join(format!("oaip_v1_migrate_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let db = libsql::Builder::new_local(path.to_str().unwrap())
            .build()
            .await
            .unwrap();
        let conn = db.connect().unwrap();
        // 造 V1 库：对象表 + wb_meta(version=1) + 缺新列的 wb_learned
        conn.execute_batch(
            "CREATE TABLE wb_ontology_object (
               id TEXT PRIMARY KEY, create_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
               update_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, is_delete INTEGER NOT NULL DEFAULT 0,
               type TEXT NOT NULL, data TEXT NOT NULL, version INTEGER NOT NULL DEFAULT 1);
             CREATE TABLE wb_meta (key TEXT PRIMARY KEY, value TEXT);
             INSERT INTO wb_meta (key, value) VALUES ('schema_version', '1');
             CREATE TABLE wb_learned (
               id TEXT PRIMARY KEY, kind TEXT NOT NULL, action TEXT NOT NULL,
               confidence REAL NOT NULL, proposal_id TEXT NOT NULL, run_ids TEXT NOT NULL,
               created_at TEXT NOT NULL);",
        )
        .await
        .unwrap();
        drop(conn);

        // open 应自动迁移：不报 WB2101
        let storage = LibSQLStorage::open(path.to_str().unwrap())
            .await
            .unwrap_or_else(|e| panic!("V1 库应自动迁移而非失败/要求 db reset: {e}"));
        // 迁移后 wb_learned 含新列：写入含 auto_approve_enabled 的行应成功
        storage
            .conn
            .execute(
                "INSERT INTO wb_learned (id, kind, action, confidence, proposal_id, run_ids, created_at, auto_approve_enabled) \
                 VALUES ('l1', 'ActionPattern', 'retain', 0.9, 'p1', '[]', 't', 1)",
                params![],
            )
            .await
            .unwrap();
        // 版本已推进到当前
        let mut rows = storage
            .conn
            .query(
                "SELECT value FROM wb_meta WHERE key='schema_version'",
                params![],
            )
            .await
            .unwrap();
        let v: String = rows.next().await.unwrap().unwrap().get(0).unwrap();
        assert_eq!(v, SCHEMA_VERSION.to_string(), "迁移后版本应为当前版本");
        let _ = std::fs::remove_file(&path);
    }

    // C1 迁移：版本高于程序支持 → 拒绝打开（未来版本库）
    #[tokio::test]
    async fn open_rejects_future_schema_version() {
        let path = std::env::temp_dir().join(format!("oaip_future_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let db = libsql::Builder::new_local(path.to_str().unwrap())
            .build()
            .await
            .unwrap();
        let conn = db.connect().unwrap();
        conn.execute_batch(
            "CREATE TABLE wb_ontology_object (
               id TEXT PRIMARY KEY, create_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
               update_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, is_delete INTEGER NOT NULL DEFAULT 0,
               type TEXT NOT NULL, data TEXT NOT NULL, version INTEGER NOT NULL DEFAULT 1);
             CREATE TABLE wb_meta (key TEXT PRIMARY KEY, value TEXT);
             INSERT INTO wb_meta (key, value) VALUES ('schema_version', '99');",
        )
        .await
        .unwrap();
        drop(conn);
        let err = match LibSQLStorage::open(path.to_str().unwrap()).await {
            Ok(_) => panic!("未来版本库应拒绝打开"),
            Err(e) => e,
        };
        let _ = std::fs::remove_file(&path);
        match err {
            StorageError::Constraint { code, .. } => assert_eq!(code, "WB2101"),
            other => panic!("应返回 WB2101，实际: {other:?}"),
        }
    }
}
