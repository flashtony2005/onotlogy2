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

    /// 渲染对象列表为 `id name level` 行（对应 TC-006）。
    pub fn render_object_list(records: &[ObjectRecord]) -> String {
        let mut lines = Vec::new();
        for r in records {
            let name = r.data.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let level = r.data.get("level").and_then(|v| v.as_str()).unwrap_or("");
            lines.push(format!("{} {} {}", r.id, name, level));
        }
        lines.join("\n")
    }
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
