//! 存储层 —— ObjectEngine 抽象 + LibSQLStorage（Turso 落地）（对应 REQ-004 / REQ-005）
//!
//! 对象统一存 `wb_ontology_object`（语义ID）；TraceEvent 统一存 `wb_trace_event`。
//! 使用 libSQL 0.9 异步 API。列名与 sql-script.md 权威 DDL 一致（type / source / relation / target）。
//! 另用 `wb_meta.schema_version` 记录 schema 版本，open 据此拦截旧库（WB2101）。
//!
//! M4：独立 crate（依赖 abi/trace/eval-core/mutation-core/worker-core——三个类型环已解）。

pub mod evaluation;
pub mod events;
pub mod objects;
pub mod projects;
pub mod relations;
pub mod runs;
pub mod users;
pub mod worker_registry;
pub mod worker_stats;
pub mod world;

use async_trait::async_trait;
use libsql::Connection;
use libsql::params;
use oaip_abi::codegen;
pub(crate) use oaip_trace::TraceEvent;
pub use projects::ProjectRecord;
pub use runs::{ApprovalRecord, RunRecord};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
pub use world::WorldCommand;

static REL_SEQ: AtomicU64 = AtomicU64::new(0);

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

/// 持久化世界状态快照（P3 增量投影：当前关系 + watermark，存于 `wb_meta` JSON blob）。
///
/// 设计取舍：世界关系规模小（百级），用 KV blob 持久化、避免新增表与 schema 迁移；
/// 投影（事件→Kernel）才是真正增量（只应用 watermark 之后的事件），这是文档 P3
/// 要解决的「每次 run 全量重放」性能问题（非正确性问题）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PersistedWorld {
    /// 最后已应用事件 id（after_id 游标）；None = 从未投影。
    pub watermark: Option<String>,
    /// 当前世界关系（valid_until 恒 None；过期关系不持久化，属历史）。
    pub relations: Vec<WorldRelation>,
}

/// 一条持久化世界关系（镜像 Kernel Relation 的当前态）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldRelation {
    pub source: String,
    pub predicate: String,
    pub target: String,
    pub valid_from: i64,
    pub valid_until: Option<i64>,
    /// 原始字段值的类型化副本（投影写入时从 Relation.metadata 镜像）；
    /// 避免 `stringify` 把数值文本化后跨 run 丢失类型（EntityView 据此还原）。
    pub metadata: Value,
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
    /// 乐观锁冲突：传入版本与当前版本不符（对象已被并发修改）。
    #[error("乐观锁冲突（期望版本 {expected}，实际不符）: {id}")]
    Conflict { id: String, expected: i64 },
}

/// Trace 事件查询条件（run/agent/object/type 过滤 + 游标 after_id 断线重连补齐）。
/// 列名取自固定白名单（run_id/agent_id/event_type/object_refs），值全部参数化绑定，防注入。
#[derive(Debug, Clone, Default)]
pub struct TraceQuery {
    /// Phase 8：项目归属过滤（None = 全部项目）。
    pub project_id: Option<String>,
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
    async fn update(
        &self,
        id: &str,
        data: Value,
        expected_version: i64,
    ) -> Result<ObjectRecord, StorageError>;
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
const SCHEMA_VERSION: i64 = 10;

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
    // V2 → V3：Phase 11 Domain Agent Evolution（WB-DOC-03-V11.0）。
    // 新增 wb_evaluation（Event Log 派生结果，可重建）与 wb_mutation（提案 + Benchmark）。
    (
        2,
        "CREATE TABLE IF NOT EXISTS wb_evaluation (
  id                 TEXT PRIMARY KEY,
  run_id             TEXT NOT NULL,
  task_id            TEXT NOT NULL,
  skill_version      TEXT,
  correctness        REAL,
  completeness       REAL,
  efficiency         REAL,
  reliability        REAL,
  policy_compliance  REAL,
  output_quality     REAL,
  aggregate_score    REAL NOT NULL DEFAULT 0.0,
  failure_modes      TEXT NOT NULL DEFAULT '[]',
  evaluator_version  TEXT NOT NULL,
  created_at         TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_eval_run ON wb_evaluation(run_id);
CREATE INDEX IF NOT EXISTS idx_eval_task ON wb_evaluation(task_id);
CREATE TABLE IF NOT EXISTS wb_mutation (
  id                 TEXT PRIMARY KEY,
  target_asset       TEXT NOT NULL,
  source_version     TEXT NOT NULL,
  candidate_version  TEXT NOT NULL,
  root_cause         TEXT NOT NULL,
  evidence           TEXT NOT NULL DEFAULT '[]',
  delta              TEXT NOT NULL,
  validation_result  TEXT,
  benchmark_result   TEXT,
  status             TEXT NOT NULL,
  promotion_reason   TEXT,
  created_at         TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_mutation_status ON wb_mutation(status);
CREATE INDEX IF NOT EXISTS idx_mutation_target ON wb_mutation(target_asset);",
    ),
    // V3 → V4：Phase 11 11A 证据系统（WB-DOC-03-V11.0）。
    // wb_evaluation 加 evidence 列（每维度证据+置信度 JSON；Unknown ≠ Success 语义）。
    (
        3,
        "ALTER TABLE wb_evaluation ADD COLUMN evidence TEXT NOT NULL DEFAULT '{}'",
    ),
    // V4 → V5：Phase 11C 离线 Benchmark（WB-DOC-03-V11.0）。
    // 新增 wb_task（任务定义持久化：历史任务集 = 按 task_id 聚合的 wb_evaluation）。
    (
        4,
        "CREATE TABLE IF NOT EXISTS wb_task (
  task_id    TEXT PRIMARY KEY,
  definition TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
)",
    ),
    // V5 → V6：V2 Router Selection Learning（Trust 轴）。
    // wb_worker_stats：(capability, worker) 执行统计（派生层，可回放重建）。
    (
        5,
        "CREATE TABLE IF NOT EXISTS wb_worker_stats (
  capability       TEXT NOT NULL,
  worker_id        TEXT NOT NULL,
  runs             INTEGER NOT NULL DEFAULT 0,
  success          INTEGER NOT NULL DEFAULT 0,
  total_duration_ms INTEGER NOT NULL DEFAULT 0,
  updated_at       TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (capability, worker_id)
)",
    ),
    // V6 → V7：V2 Router Selection Learning 可回放化。
    // wb_evaluation 加 worker_outcomes（per-worker 结果；WorkerStats 重建的数据源）。
    (
        6,
        "ALTER TABLE wb_evaluation ADD COLUMN worker_outcomes TEXT NOT NULL DEFAULT '[]'",
    ),
    // V7 → V8：S1 Decision Replay 前置——Worker Registry 持久化 + stats.first_seen。
    (
        7,
        "CREATE TABLE IF NOT EXISTS wb_worker (
  id           TEXT PRIMARY KEY,
  worker_type  TEXT NOT NULL,
  capabilities TEXT NOT NULL DEFAULT '[]',
  quality      REAL NOT NULL DEFAULT 0.0,
  trust        REAL NOT NULL DEFAULT 0.0,
  risk         INTEGER NOT NULL DEFAULT 1,
  cost         INTEGER NOT NULL DEFAULT 1,
  latency_ms   INTEGER NOT NULL DEFAULT 100,
  availability TEXT NOT NULL DEFAULT 'local',
  execution    TEXT NOT NULL DEFAULT 'Process',
  created_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
ALTER TABLE wb_worker_stats ADD COLUMN created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP;",
    ),
    // V8 → V9（M1d）：V1 门面删除——wb_memory / wb_approval / wb_pattern·proposal·learned
    // / wb_approval_audit 随 V1 存储层退役（V2 认知状态归 CognitiveStore / Kernel 事实轨）。
    (
        8,
        "DROP TABLE IF EXISTS wb_memory; DROP TABLE IF EXISTS wb_approval; \
         DROP TABLE IF EXISTS wb_pattern; DROP TABLE IF EXISTS wb_proposal; \
         DROP TABLE IF EXISTS wb_learned; DROP TABLE IF EXISTS wb_approval_audit;",
    ),
    // V9 → V10（Phase 8）：Project Scope——项目维度世界模型。
    // wb_project 项目注册表（含本体定义）；对象/关系/事件/run 全部 project 归属
    //（NULL = 全局/历史，兼容旧库；世界按 project 过滤投影）。
    (
        9,
        "CREATE TABLE IF NOT EXISTS wb_project (
  id          TEXT PRIMARY KEY,
  name        TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  ontology    TEXT NOT NULL DEFAULT '',
  status      TEXT NOT NULL DEFAULT 'active',
  created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
ALTER TABLE wb_ontology_object ADD COLUMN project_id TEXT;
ALTER TABLE wb_relation_edge ADD COLUMN project_id TEXT;
ALTER TABLE wb_trace_event ADD COLUMN project_id TEXT;
ALTER TABLE wb_run ADD COLUMN project_id TEXT;",
    ),
];

/// 判断 libsql 错误是否为 `no such table`（跨创建方稳定，优于 PRAGMA / sqlite_master 文本探测）。
fn is_no_such_table(e: &libsql::Error) -> bool {
    e.to_string().contains("no such table")
}

impl LibSQLStorage {
    /// 是否远程/本地 server 连接（libsql:// ws:// http:// https://）——Turso/libSQL server 形态。
    pub fn is_remote_url(path: &str) -> bool {
        let p = path.to_ascii_lowercase();
        p.starts_with("libsql://")
            || p.starts_with("ws://")
            || p.starts_with("wss://")
            || p.starts_with("http://")
            || p.starts_with("https://")
    }

    /// 打开（或创建）数据库并初始化核心表，含旧 schema 拦截（WB2101）。
    ///
    /// - 本地路径 / `:memory:` → 本地文件形态
    /// - `libsql://` / `ws://` / `http(s)://` → 连接 Turso/libSQL **server**（本地或远端），
    ///   auth token 从环境变量 `TURSO_AUTH_TOKEN` 读取（本地 dev server 可为空）
    pub async fn open(path: &str) -> Result<Self, StorageError> {
        if Self::is_remote_url(path) {
            let token = std::env::var("TURSO_AUTH_TOKEN").ok();
            return Self::open_remote(path, token.as_deref()).await;
        }
        let db = libsql::Builder::new_local(path).build().await?;
        let conn = db.connect()?;
        Self::ensure_schema(&conn).await?;
        Ok(Self { conn })
    }

    /// 显式连接 Turso/libSQL server（URL + 可选 auth token）。
    pub async fn open_remote(url: &str, token: Option<&str>) -> Result<Self, StorageError> {
        let builder = libsql::Builder::new_remote(url.to_string(), token.unwrap_or("").to_string());
        let db = builder.build().await.map_err(StorageError::Db)?;
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

    /// 读 KV 元数据（复用 `wb_meta`；P3 watermark / 世界投影状态载体）。
    pub async fn get_meta(&self, key: &str) -> Result<Option<String>, StorageError> {
        let mut rows = self
            .conn
            .query("SELECT value FROM wb_meta WHERE key = ?1", params![key])
            .await?;
        match rows.next().await? {
            Some(r) => Ok(Some(r.get::<String>(0)?)),
            None => Ok(None),
        }
    }

    /// 写 KV 元数据（幂等覆盖；`wb_meta` 以 key 为主键）。
    pub async fn set_meta(&self, key: &str, value: &str) -> Result<(), StorageError> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO wb_meta (key, value) VALUES (?1, ?2)",
                params![key, value],
            )
            .await?;
        Ok(())
    }

    /// 读持久化世界状态（P3）：watermark + 当前关系。无记录返回空（首轮全量）。
    pub async fn load_projection(&self, world_key: &str) -> Result<PersistedWorld, StorageError> {
        match self.get_meta(&format!("projection:{world_key}")).await? {
            Some(s) => Ok(serde_json::from_str(&s)?),
            None => Ok(PersistedWorld::default()),
        }
    }

    /// 写持久化世界状态（P3）：覆盖式保存当前关系 + watermark。
    pub async fn save_projection(
        &self,
        world_key: &str,
        world: &PersistedWorld,
    ) -> Result<(), StorageError> {
        let s = serde_json::to_string(world)?;
        self.set_meta(&format!("projection:{world_key}"), &s).await
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
    let project_id: Option<String> = row.get(4)?;
    let run_id: Option<String> = row.get(5)?;
    let agent_id: Option<String> = row.get(6)?;
    let reasoning: Option<String> = row.get(7)?;
    let input: Option<String> = row.get(8)?;
    let output: Option<String> = row.get(9)?;
    let confidence: Option<f64> = row.get(10)?;
    let kind_str: String = row.get(1)?;
    Ok(TraceEvent {
        id: row.get(0)?,
        kind: oaip_trace::TraceKind::from_str(&kind_str).unwrap_or(oaip_trace::TraceKind::World),
        event_type: row.get(2)?,
        object_refs: serde_json::from_str(&obj_refs)?,
        project_id,
        run_id,
        agent_id,
        reasoning,
        input: input.map(|s| serde_json::from_str(&s)).transpose()?,
        output: output.map(|s| serde_json::from_str(&s)).transpose()?,
        confidence,
        timestamp: row.get(11)?,
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

    // ── Turso/libSQL server 远程形态 ────────────────────────────────────
    #[test]
    fn remote_url_classification() {
        // 远程/本地 server：libsql:// ws:// http(s)://
        for url in [
            "libsql://127.0.0.1:8080",
            "libsql://localhost:8080",
            "ws://127.0.0.1:8080",
            "wss://host.example",
            "http://127.0.0.1:8080",
            "https://db.turso.io",
        ] {
            assert!(
                LibSQLStorage::is_remote_url(url),
                "{url} 应识别为 server 连接"
            );
        }
        // 本地文件 / 内存
        for p in [":memory:", "/tmp/a.db", "data.db", "file:db?mode=ro"] {
            assert!(!LibSQLStorage::is_remote_url(p), "{p} 应为本地形态");
        }
    }

    #[test]
    fn remote_url_case_insensitive() {
        assert!(LibSQLStorage::is_remote_url("LIBSQL://127.0.0.1:8080"));
        assert!(LibSQLStorage::is_remote_url("HTTPS://db.turso.io"));
    }

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

    // 乐观锁：并发 update 同一对象，仅一个成功、另一个版本冲突（杜绝丢失更新）
    #[tokio::test]
    async fn concurrent_updates_optimistic_lock_prevents_lost_update() {
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
        // 两方都基于读到的版本 1 并发写
        let (a, b) = tokio::join!(
            async {
                s1.update("001", serde_json::json!({"name": "A", "level": "GOLD"}), 1)
                    .await
            },
            async {
                s2.update(
                    "001",
                    serde_json::json!({"name": "A", "level": "PLATINUM"}),
                    1,
                )
                .await
            },
        );
        // 恰一个成功（version→2），另一个 Conflict（版本不符）
        let ok_count = [a.is_ok(), b.is_ok()].iter().filter(|x| **x).count();
        assert_eq!(ok_count, 1, "并发 update 恰一个成功（乐观锁）");
        assert!(a.is_err() || b.is_err(), "另一个必须版本冲突（无丢失更新）");
        let rec = s.get("001").await.unwrap().unwrap();
        assert_eq!(rec.version, 2, "最终版本应为 2（仅一次更新生效）");
        // 基于最新版本（2）重试应成功
        let retried = s
            .update(
                "001",
                serde_json::json!({"name": "A", "level": "RETRY"}),
                rec.version,
            )
            .await;
        assert!(retried.is_ok(), "基于最新版本重试应成功");
        assert_eq!(retried.unwrap().version, 3);
    }

    // 并发 link 同一对边：两条独立连接 → 事务竞争；`uq_edge_active` 部分唯一索引
    // 作硬保证，恰一条成功、另一条 WB2007，不得双插（link TOCTOU 回归）。
    // 注意：`:memory:` + clone 共享单连接，无法真正并发；故用同一文件的两条连接。
    #[tokio::test]
    async fn concurrent_link_same_edge_no_duplicate() {
        let path = std::env::temp_dir().join(format!(
            "oaip_link_{}_{}.db",
            std::process::id(),
            "concurrent"
        ));
        let _ = std::fs::remove_file(&path);
        let s = LibSQLStorage::open(path.to_str().unwrap()).await.unwrap();
        s.create("Customer", "001", serde_json::json!({"name": "A"}))
            .await
            .unwrap();
        s.create("Task", "t1", serde_json::json!({"title": "T"}))
            .await
            .unwrap();
        // 两条独立连接到同一文件 → 真正并发写入（unique 索引仲裁）
        let s1 = LibSQLStorage::open(path.to_str().unwrap()).await.unwrap();
        let s2 = LibSQLStorage::open(path.to_str().unwrap()).await.unwrap();
        let (r1, r2) = tokio::join!(async { s1.link("001", "owns", "t1", None).await }, async {
            s2.link("001", "owns", "t1", None).await
        });
        let ok_count = [r1.is_ok(), r2.is_ok()].iter().filter(|b| **b).count();
        assert_eq!(ok_count, 1, "并发 link 应恰一条成功");
        let dup = [r1, r2].into_iter().find_map(|r| match r {
            Err(StorageError::Constraint { code, .. }) => Some(code.to_string()),
            _ => None,
        });
        assert_eq!(dup.as_deref(), Some("WB2007"), "另一条应拒 WB2007");
        let edges = s1.neighbors("001").await.unwrap();
        assert_eq!(edges.len(), 1, "不得双插同一条边");
        let _ = std::fs::remove_file(&path);
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
               created_at TEXT NOT NULL);
             CREATE TABLE wb_relation_edge (
               id TEXT PRIMARY KEY, create_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
               update_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, is_delete INTEGER NOT NULL DEFAULT 0,
               relation TEXT NOT NULL, source TEXT NOT NULL, target TEXT NOT NULL, data TEXT);
             CREATE TABLE wb_trace_event (
               id TEXT PRIMARY KEY, create_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
               update_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, is_delete INTEGER NOT NULL DEFAULT 0,
               kind TEXT NOT NULL, run_id TEXT, agent_id TEXT, event_type TEXT NOT NULL,
               object_refs TEXT, reasoning TEXT, input TEXT, output TEXT, confidence REAL, timestamp TEXT NOT NULL);
             CREATE TABLE wb_run (
               id TEXT PRIMARY KEY, agent_id TEXT NOT NULL, goal TEXT NOT NULL,
               status TEXT NOT NULL, human_approval INTEGER NOT NULL DEFAULT 0,
               created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
               updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, finished_at TEXT);",
        )
        .await
        .unwrap();
        drop(conn);

        // open 应自动迁移：不报 WB2101
        let storage = LibSQLStorage::open(path.to_str().unwrap())
            .await
            .unwrap_or_else(|e| panic!("V1 库应自动迁移而非失败/要求 db reset: {e}"));
        // M1d：V1 表（wb_learned 等）已被迁移删除——再建同名表不应冲突，且旧表确实不在
        let mut missing = storage
            .conn
            .query(
                "SELECT name FROM sqlite_master WHERE type='table' AND name IN ('wb_learned','wb_memory','wb_approval')",
                params![],
            )
            .await
            .unwrap();
        assert!(
            missing.next().await.unwrap().is_none(),
            "V1 表应在迁移中被 DROP"
        );
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
