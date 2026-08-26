//! 存储层 —— ObjectEngine 抽象 + LibSQLStorage（Turso 落地）（对应 REQ-004 / REQ-005）
//!
//! 对象统一存 `wb_ontology_object`（语义ID）；TraceEvent 统一存 `wb_trace_event`。
//! 使用 libSQL 0.9 异步 API。列名与 sql-script.md 权威 DDL 一致（type / source / relation / target）。
//! 另用 `wb_meta.schema_version` 记录 schema 版本，open 据此拦截旧库（WB2101）。
//!
//! M4：独立 crate（依赖 abi/trace/eval-core/mutation-core/worker-core——三个类型环已解）。

pub mod automation;
pub mod evaluation;
pub mod events;
pub mod leases;
pub mod objects;
pub use ontology::{OntologyEvolutionRecord, OntologyEvolutionStatus, OntologyVersionRecord};
pub mod ontology;
pub mod optimization;
pub mod projects;
pub mod relations;
pub mod resources;
pub mod runs;
pub mod users;
pub mod worker_registry;
pub mod worker_stats;
pub mod world;
pub use automation::AutomationJob;
pub use leases::ExecutionLeaseRecord;
pub use optimization::BusinessOptimizationRunRecord;

mod backup;
mod schema;

use async_trait::async_trait;
use libsql::Connection;
use libsql::params;
pub(crate) use oaip_trace::TraceEvent;
use tokio::sync::Mutex;

/// 审计事件哈希链中的持久化节点。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditChainRecord {
    pub sequence: i64,
    pub event_id: String,
    pub previous_hash: String,
    pub hash: String,
}

pub use projects::ProjectRecord;
pub use runs::RunRecord;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::time::Duration;
pub use world::WorldCommand;

static REL_SEQ: AtomicU64 = AtomicU64::new(0);

/// 审计链健康状态：启动深校验成功后才为 Valid；热路径只在检测到链损坏时降级。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditChainHealth {
    Unknown,
    Valid,
    Invalid,
}

impl AuditChainHealth {
    pub(crate) const fn code(self) -> u8 {
        match self {
            Self::Unknown => 0,
            Self::Valid => 1,
            Self::Invalid => 2,
        }
    }

    pub(crate) const fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Valid,
            2 => Self::Invalid,
            _ => Self::Unknown,
        }
    }
}

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

/// 测试专用故障注入点。
///
/// 仅在 storage crate 的单元测试编译时存在；子进程通过环境变量在事务提交前
/// 确定性终止，用于验证进程崩溃后的原子性与重启恢复，不进入生产二进制。
#[cfg(test)]
pub(crate) fn crash_if_requested(point: &str) {
    if std::env::var("OAIP_STORAGE_CRASH_POINT").ok().as_deref() == Some(point) {
        std::process::abort();
    }
}

/// 测试专用持久化故障注入点。
///
/// 格式：`OAIP_STORAGE_IO_FAULT=<point>:<readonly|full>`。只在 storage crate
/// 单元测试中编译，用 SQLite 原生错误码模拟宿主机只读或磁盘耗尽，验证事务
/// 回滚与上层错误传播；生产二进制不包含该路径。
#[cfg(test)]
pub(crate) fn io_fault_if_requested(point: &str) -> Result<(), StorageError> {
    let Some(value) = std::env::var("OAIP_STORAGE_IO_FAULT").ok() else {
        return Ok(());
    };
    let Some((requested_point, mode)) = value.split_once(':') else {
        return Ok(());
    };
    if requested_point != point {
        return Ok(());
    }
    let (code, message) = match mode {
        "readonly" => (8, "attempt to write a readonly database"),
        "full" => (13, "database or disk is full"),
        _ => return Ok(()),
    };
    Err(StorageError::Db(libsql::Error::SqliteFailure(
        code,
        message.into(),
    )))
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
    #[error("ODL 编译错误: {0}")]
    Odl(#[from] oaip_abi::odl::OdlError),
    #[error("对象不存在: {0}")]
    NotFound(String),
    #[error("序列化错误: {0}")]
    Json(#[from] serde_json::Error),
    #[error("约束违反 {code}: {message}")]
    Constraint { code: &'static str, message: String },
    #[error("文件系统错误: {0}")]
    Io(#[from] std::io::Error),
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
    /// 倒序返回（timestamp DESC）：审计/CLI 轮询取「最新」事件时用，避免
    /// ASC LIMIT 只取最旧 N 条导致终局事件（最新）被窗口截断（C2 修复）。
    /// WS 断线重连补齐（after_id 游标）必须 ASC，默认 false。
    pub newest_first: bool,
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
    audit_health: Arc<AtomicU8>,
    /// libSQL 的一个逻辑连接不能同时持有多个事务。所有进程内写事务共享这把锁，
    /// 避免并发 API 请求把事务状态写到同一个连接上。
    write_txn_lock: Arc<Mutex<()>>,
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
        conn.busy_timeout(Duration::from_secs(5))
            .map_err(StorageError::Db)?;
        Self::ensure_schema(&conn).await?;
        let storage = Self {
            conn,
            audit_health: Arc::new(AtomicU8::new(AuditChainHealth::Unknown.code())),
            write_txn_lock: Arc::new(Mutex::new(())),
        };
        storage.ensure_audit_chain().await?;
        seed_trace_clock(&storage.conn).await?;
        Ok(storage)
    }

    /// 显式连接 Turso/libSQL server（URL + 可选 auth token）。
    pub async fn open_remote(url: &str, token: Option<&str>) -> Result<Self, StorageError> {
        let builder = libsql::Builder::new_remote(url.to_string(), token.unwrap_or("").to_string());
        let db = builder.build().await.map_err(StorageError::Db)?;
        let conn = db.connect()?;
        conn.busy_timeout(Duration::from_secs(5))
            .map_err(StorageError::Db)?;
        Self::ensure_schema(&conn).await?;
        let storage = Self {
            conn,
            audit_health: Arc::new(AtomicU8::new(AuditChainHealth::Unknown.code())),
            write_txn_lock: Arc::new(Mutex::new(())),
        };
        storage.ensure_audit_chain().await?;
        seed_trace_clock(&storage.conn).await?;
        Ok(storage)
    }

    /// 生成一致性本地快照。目标文件必须不存在，避免静默覆盖备份。
    pub async fn backup_to(
        &self,
        destination: impl AsRef<std::path::Path>,
    ) -> Result<(), StorageError> {
        backup::backup(&self.conn, destination).await
    }

    /// 校验快照后，以同目录原子替换安装到本地数据库路径。
    pub async fn restore_from(
        source: impl AsRef<std::path::Path>,
        destination: impl AsRef<std::path::Path>,
    ) -> Result<(), StorageError> {
        backup::restore(source, destination).await
    }

    /// 初始化连接：交给 schema 模块完成版本化迁移与 canonical 校验。
    async fn ensure_schema(conn: &Connection) -> Result<(), StorageError> {
        schema::ensure_schema(conn).await
    }

    /// 从已有连接构造（测试可用）。
    pub fn from_connection(conn: Connection) -> Self {
        Self {
            conn,
            audit_health: Arc::new(AtomicU8::new(AuditChainHealth::Unknown.code())),
            write_txn_lock: Arc::new(Mutex::new(())),
        }
    }

    /// 克隆共享连接，测试可模拟多个服务实例访问同一数据库。
    #[doc(hidden)]
    pub fn test_shared_connection(&self) -> Self {
        Self::from_connection(self.conn.clone())
    }

    /// 返回最近一次启动/深度校验与热路径校验得到的审计健康状态。
    pub fn audit_chain_health(&self) -> AuditChainHealth {
        AuditChainHealth::from_code(self.audit_health.load(Ordering::Acquire))
    }

    pub(crate) fn mark_audit_chain_valid(&self) {
        self.audit_health
            .store(AuditChainHealth::Valid.code(), Ordering::Release);
    }

    pub(crate) fn mark_audit_chain_invalid(&self) {
        self.audit_health
            .store(AuditChainHealth::Invalid.code(), Ordering::Release);
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

async fn seed_trace_clock(conn: &Connection) -> Result<(), StorageError> {
    let mut rows = conn
        .query(
            "SELECT timestamp FROM wb_trace_event ORDER BY timestamp DESC, id DESC LIMIT 1",
            params![],
        )
        .await?;
    if let Some(row) = rows.next().await? {
        let timestamp: String = row.get(0)?;
        oaip_trace::observe_timestamp(&timestamp);
    }
    Ok(())
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
    async fn restart_seeds_trace_clock_from_persisted_events() {
        let path = std::env::temp_dir().join(format!(
            "oaip_clock_{}_{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        remove_crash_db(&path);
        let latest = (oaip_trace::now_ts().parse::<u64>().unwrap() + 10_000).to_string();
        {
            let storage = LibSQLStorage::open(path.to_str().unwrap()).await.unwrap();
            storage
                .append_trace(&TraceEvent {
                    id: "clock-seed".into(),
                    kind: oaip_trace::TraceKind::World,
                    event_type: "ClockSeed".into(),
                    object_refs: vec!["clock".into()],
                    project_id: None,
                    run_id: None,
                    agent_id: None,
                    reasoning: None,
                    input: None,
                    output: None,
                    confidence: None,
                    timestamp: latest.clone(),
                })
                .await
                .unwrap();
        }

        let _storage = LibSQLStorage::open(path.to_str().unwrap()).await.unwrap();
        let next = oaip_trace::now_ts().parse::<u64>().unwrap();
        assert!(next > latest.parse::<u64>().unwrap());
        remove_crash_db(&path);
    }

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

    // 同一个 libSQL 连接不能同时开启两个事务。这里用克隆出的同一存储句柄
    // 并发提交世界变更，锁定事务入口，防止 HTTP 并发触发连接级事务冲突。
    #[tokio::test]
    async fn concurrent_world_commands_share_one_transaction_entry() {
        let s = LibSQLStorage::open(":memory:").await.unwrap();
        let mut tasks = tokio::task::JoinSet::new();
        for idx in 0..24 {
            let s = s.clone();
            tasks.spawn(async move {
                s.apply_world(world::WorldCommand::CreateObject {
                    object_type: "Customer".into(),
                    id: format!("concurrent-{idx}"),
                    data: serde_json::json!({"name": idx}),
                    project_id: None,
                })
                .await
            });
        }
        while let Some(result) = tasks.join_next().await {
            result.unwrap().unwrap();
        }
        let objects = s.query("Customer").await.unwrap();
        assert_eq!(objects.len(), 24);
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
        assert_eq!(
            v,
            super::schema::SCHEMA_VERSION.to_string(),
            "迁移后版本应为当前版本"
        );
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

    // ── C2 回归：审计倒序（newest_first）让 CLI 异步轮询取到最新终局事件 ──
    // 修复前 list_events 固定 ORDER BY timestamp ASC LIMIT 500 → 只返回最旧 500 条，
    // 终局 fabric 事件（最新）永远不在窗口 → CLI 每 100ms 轮询同一旧窗口 → 30s
    // 报「超时未终局」→ 进程退出杀掉后台任务。修复：审计域 newest_first=true
    // 返回最新 500 条（含终局）；WS 断线补齐仍走 after_id 游标（ASC）不受影响。
    #[tokio::test]
    async fn list_events_newest_first_exposes_terminal_event_over_limit() {
        let s = LibSQLStorage::open(":memory:").await.unwrap();
        // 超过 500 条 fabric.* 事件，时间戳单调递增；最新一条是终局
        // fabric.task_completed（CLI 异步轮询要等的就是它）。
        let total = 600usize;
        for i in 1..=total {
            let ev = TraceEvent {
                id: format!("tr_{i:04}"),
                kind: oaip_trace::TraceKind::Cognitive,
                event_type: if i == total {
                    "fabric.task_completed".into()
                } else {
                    "fabric.progress".into()
                },
                object_refs: vec!["c-1".into()],
                project_id: None,
                run_id: Some("run-1".into()),
                agent_id: None,
                reasoning: None,
                input: None,
                output: None,
                confidence: None,
                timestamp: format!("{i:020}"), // 零填充定长 → 字典序=数值序
            };
            s.append_trace(&ev).await.unwrap();
        }

        // 修复前语义（newest_first=false）：窗口是最旧 500 条 → 终局缺失
        let asc = s
            .list_events(TraceQuery {
                limit: 500,
                newest_first: false,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(asc.len(), 500, "ASC LIMIT 500 返回最旧 500 条");
        assert!(
            !asc.iter().any(|e| e.event_type == "fabric.task_completed"),
            "修复前：终局事件（最新）在最旧 500 窗口之外 → CLI 漏终局"
        );

        // 修复后语义（newest_first=true）：返回最新 500 条，终局在前
        let desc = s
            .list_events(TraceQuery {
                limit: 500,
                newest_first: true,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(desc.len(), 500, "DESC LIMIT 500 返回最新 500 条");
        assert_eq!(
            desc[0].event_type, "fabric.task_completed",
            "修复后：终局事件（最新）必须出现在窗口首位"
        );

        // WS 断线补齐仍走 after_id 游标（ASC），不被 newest_first 影响
        let cursor = s
            .list_events(TraceQuery {
                after_id: Some("tr_0001".into()),
                limit: 1000,
                newest_first: false,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(
            cursor.len(),
            total - 1,
            "after_id 游标仍按 ASC 补齐后续事件（WS 断线重连语义不变）"
        );
    }

    /// S1e：用 SQLite 原生 SQLITE_READONLY/SQLITE_FULL 错误码模拟宿主机故障。
    ///
    /// 故障点由环境变量确定，子进程只执行一次写入，父进程随后重启数据库验证：
    /// 事务没有留下孤立 world event、对象或 trace。这样不依赖真实磁盘配额，
    /// 也不把测试故障注入路径编译进生产二进制。
    #[tokio::test]
    async fn io_fault_probe() {
        let Some(fault) = std::env::var_os("OAIP_STORAGE_IO_FAULT") else {
            return;
        };
        let db = std::env::var("OAIP_STORAGE_IO_FAULT_DB").unwrap();
        let operation = std::env::var("OAIP_STORAGE_IO_FAULT_OPERATION").unwrap();
        let fault = fault.to_string_lossy();
        let (_, mode) = fault.split_once(':').unwrap();
        let storage = LibSQLStorage::open(&db).await.unwrap();
        let error = match operation.as_str() {
            "world" => storage
                .apply_world(WorldCommand::CreateObject {
                    object_type: "Customer".into(),
                    id: "io-fault-object".into(),
                    data: serde_json::json!({"name": "io-fault"}),
                    project_id: None,
                })
                .await
                .expect_err("故障注入必须阻断 world 写入"),
            "trace" => storage
                .append_trace(&TraceEvent {
                    id: "io-fault-trace".into(),
                    kind: oaip_trace::TraceKind::World,
                    event_type: "ObjectCreated".into(),
                    object_refs: vec!["io-fault-object".into()],
                    project_id: None,
                    run_id: None,
                    agent_id: None,
                    reasoning: None,
                    input: None,
                    output: None,
                    confidence: None,
                    timestamp: "io-fault".into(),
                })
                .await
                .expect_err("故障注入必须阻断 trace 写入"),
            other => panic!("unknown io fault operation: {other}"),
        };
        let expected_code = match mode {
            "readonly" => 8,
            "full" => 13,
            other => panic!("unknown io fault mode: {other}"),
        };
        match error {
            StorageError::Db(libsql::Error::SqliteFailure(code, message)) => {
                assert_eq!(code, expected_code);
                assert!(message.contains(mode), "错误信息应保留故障语义: {message}");
            }
            other => panic!("故障必须保留 SQLite 错误码，实际: {other:?}"),
        }
    }

    fn run_io_fault_probe(path: &std::path::Path, operation: &str, point: &str, mode: &str) {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("tests::io_fault_probe")
            .arg("--nocapture")
            .env("OAIP_STORAGE_IO_FAULT_DB", path)
            .env("OAIP_STORAGE_IO_FAULT_OPERATION", operation)
            .env("OAIP_STORAGE_IO_FAULT", format!("{point}:{mode}"))
            .status()
            .expect("启动 IO fault probe");
        assert!(status.success(), "IO fault probe 应识别 {point}:{mode}");
    }

    #[tokio::test]
    async fn io_faults_preserve_world_and_trace_atomicity() {
        for mode in ["readonly", "full"] {
            for point in ["world.after_event_insert", "world.after_state_write"] {
                let path = crash_db_path(&format!("io_{mode}_{point}"));
                run_io_fault_probe(&path, "world", point, mode);
                let storage = LibSQLStorage::open(path.to_str().unwrap()).await.unwrap();
                assert!(
                    storage.list_world_events().await.unwrap().is_empty(),
                    "{point}:{mode}: 不得留下孤立 world event"
                );
                assert!(
                    storage.get("io-fault-object").await.unwrap().is_none(),
                    "{point}:{mode}: 不得留下孤立对象"
                );
                remove_crash_db(&path);
            }

            let path = crash_db_path(&format!("io_trace_{mode}"));
            run_io_fault_probe(&path, "trace", "trace.before_insert", mode);
            let storage = LibSQLStorage::open(path.to_str().unwrap()).await.unwrap();
            assert!(
                storage
                    .list_events(TraceQuery {
                        object_ref: Some("io-fault-object".into()),
                        ..Default::default()
                    })
                    .await
                    .unwrap()
                    .is_empty(),
                "trace.before_insert:{mode}: 不得留下 trace"
            );
            remove_crash_db(&path);
        }
    }

    /// S1c₀：子进程在事务提交前崩溃，重启后验证事件与对象不会半写。
    ///
    /// 该测试使用当前测试二进制作为 crash probe，故障点由环境变量精确选择；
    /// 不依赖 sleep/竞态，也不把测试注入路径编译进生产二进制。
    #[tokio::test]
    async fn crash_probe() {
        let Some(point) = std::env::var_os("OAIP_STORAGE_CRASH_POINT") else {
            return;
        };
        let db = std::env::var("OAIP_STORAGE_CRASH_DB").unwrap();
        let operation = std::env::var("OAIP_STORAGE_CRASH_OPERATION").unwrap();
        let storage = LibSQLStorage::open(&db).await.unwrap();
        match operation.as_str() {
            "world" => {
                storage
                    .apply_world(WorldCommand::CreateObject {
                        object_type: "Customer".into(),
                        id: "crash-object".into(),
                        data: serde_json::json!({"name": "crash-probe"}),
                        project_id: None,
                    })
                    .await
                    .unwrap();
            }
            "run" => storage
                .insert_run("crash-run", "probe-agent", "probe")
                .await
                .unwrap(),
            other => panic!("unknown crash operation: {other}"),
        }
        panic!("fault point {point:?} did not terminate the probe");
    }

    fn crash_db_path(label: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "oaip_s1c0_{label}_{}_{}.db",
            std::process::id(),
            nonce
        ))
    }

    fn remove_crash_db(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }

    fn run_crash_probe(path: &std::path::Path, operation: &str, point: &str) {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("tests::crash_probe")
            .arg("--nocapture")
            .env("OAIP_STORAGE_CRASH_DB", path)
            .env("OAIP_STORAGE_CRASH_OPERATION", operation)
            .env("OAIP_STORAGE_CRASH_POINT", point)
            .status()
            .expect("启动 crash probe");
        assert!(!status.success(), "crash probe 应在 {point} 处终止");
    }

    #[tokio::test]
    async fn crash_injection_preserves_atomic_world_and_recovers_runtime_states() {
        for point in ["world.after_event_insert", "world.after_state_write"] {
            let path = crash_db_path("world");
            run_crash_probe(&path, "world", point);
            let storage = LibSQLStorage::open(path.to_str().unwrap()).await.unwrap();
            assert!(
                storage.list_world_events().await.unwrap().is_empty(),
                "{point}: 崩溃后不得留下孤立 world event"
            );
            assert!(
                storage.get("crash-object").await.unwrap().is_none(),
                "{point}: 崩溃后不得留下孤立对象"
            );
            remove_crash_db(&path);
        }

        let path = crash_db_path("run");
        run_crash_probe(&path, "run", "run.after_insert");
        let storage = LibSQLStorage::open(path.to_str().unwrap()).await.unwrap();
        let runs = storage.recover_persisted_state().await.unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, "Interrupted");
        remove_crash_db(&path);
    }
}
