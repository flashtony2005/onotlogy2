//! Persistence —— Runtime 状态 + 事件历史的持久化（Phase 1：全量快照式）。
//!
//! 五张核心表（runtime-kernel-v2.md §28）：
//! contexts / fibers / effects / relations / events
//!
//! Phase 1 语义：`save` 全量快照、`restore` 重建 Kernel（含 id 计数器）。
//! 增量事件流 / Event Sourcing / replay 属 Phase 4（Temporal World Model）。

use libsql::{Builder, Connection, params};
use serde_json::Value;

use super::effect::{Effect, EffectState, Reversibility};
use super::fiber::{Fiber, FiberState, WaitPattern};
use super::relation::Relation;
use super::{
    Compensation, Context, ContextId, ContextState, EffectId, FiberId, Kernel, KernelEvent,
    RelationId, Scope,
};

/// 持久化错误。
#[derive(Debug, thiserror::Error)]
pub enum PersistError {
    #[error("数据库错误: {0}")]
    Db(#[from] libsql::Error),
    #[error("序列化错误: {0}")]
    Json(#[from] serde_json::Error),
    #[error("恢复数据损坏: {0}")]
    Corrupt(String),
}

const DDL: &str = r#"
CREATE TABLE IF NOT EXISTS contexts (
  id TEXT PRIMARY KEY, parent_id TEXT, entity_id TEXT NOT NULL, scope TEXT NOT NULL,
  state TEXT NOT NULL, valid_from INTEGER NOT NULL, valid_until INTEGER,
  state_json TEXT NOT NULL DEFAULT '{}', created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS fibers (
  id TEXT PRIMARY KEY, context_id TEXT NOT NULL, parent_id TEXT, state TEXT NOT NULL,
  task TEXT NOT NULL, input_json TEXT NOT NULL, output_json TEXT,
  wait_source TEXT, wait_predicate TEXT, wait_target TEXT, created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS effects (
  id TEXT PRIMARY KEY, fiber_id TEXT NOT NULL, action TEXT NOT NULL,
  input_json TEXT NOT NULL, output_json TEXT, status TEXT NOT NULL,
  reversibility TEXT NOT NULL, inverse_action TEXT, inverse_json TEXT, timestamp INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS relations (
  id TEXT PRIMARY KEY, context_id TEXT NOT NULL, source TEXT NOT NULL,
  predicate TEXT NOT NULL, target TEXT NOT NULL,
  valid_from INTEGER NOT NULL, valid_until INTEGER, metadata_json TEXT NOT NULL DEFAULT '{}'
);
CREATE TABLE IF NOT EXISTS events (
  id INTEGER PRIMARY KEY AUTOINCREMENT, context_id TEXT, fiber_id TEXT,
  type TEXT NOT NULL, payload_json TEXT NOT NULL, timestamp INTEGER NOT NULL
);
"#;

/// 持久化句柄（独立连接；Kernel 可随时 save/restore）。
#[derive(Clone)]
pub struct Persistence {
    conn: Connection,
}

impl Persistence {
    pub async fn open(path: &str) -> Result<Self, PersistError> {
        let db = Builder::new_local(path).build().await?;
        let conn = db.connect()?;
        conn.execute_batch(DDL).await?;
        Ok(Self { conn })
    }

    /// 全量快照保存（Phase 1 简化：整库写一遍；增量事件流属 Phase 4）。
    pub async fn save(&self, kernel: &Kernel) -> Result<(), PersistError> {
        let tx = self.conn.transaction().await?;
        tx.execute("DELETE FROM contexts", params![]).await?;
        tx.execute("DELETE FROM fibers", params![]).await?;
        tx.execute("DELETE FROM effects", params![]).await?;
        tx.execute("DELETE FROM relations", params![]).await?;
        tx.execute("DELETE FROM events", params![]).await?;

        for c in kernel.all_contexts() {
            tx.execute(
                "INSERT INTO contexts (id, parent_id, entity_id, scope, state, valid_from, valid_until, state_json, created_at) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![
                    c.id.to_string(),
                    c.parent.map(|p| p.to_string()),
                    c.entity_id.clone(),
                    scope_str(c.scope),
                    context_state_str(c.state),
                    c.valid_from,
                    c.valid_until,
                    serde_json::to_string(&c.state_store.snapshot())?,
                    c.valid_from,
                ],
            )
            .await?;
        }
        for f in kernel.all_fibers() {
            tx.execute(
                "INSERT INTO fibers (id, context_id, parent_id, state, task, input_json, output_json, wait_source, wait_predicate, wait_target, created_at) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                params![
                    f.id.to_string(),
                    f.context.to_string(),
                    f.parent.map(|p| p.to_string()),
                    f.state.as_str(),
                    f.task.clone(),
                    serde_json::to_string(&f.input)?,
                    f.output.as_ref().map(serde_json::to_string).transpose()?,
                    f.wait_for.as_ref().and_then(|w| w.source.clone()),
                    f.wait_for.as_ref().map(|w| w.predicate.clone()),
                    f.wait_for.as_ref().and_then(|w| w.target.clone()),
                    f.created_at,
                ],
            )
            .await?;
        }
        for e in kernel.all_effects() {
            tx.execute(
                "INSERT INTO effects (id, fiber_id, action, input_json, output_json, status, reversibility, inverse_action, inverse_json, timestamp) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![
                    e.id.to_string(),
                    e.fiber.to_string(),
                    e.action.clone(),
                    serde_json::to_string(&e.input)?,
                    e.output.as_ref().map(serde_json::to_string).transpose()?,
                    effect_state_str(e.status),
                    reversibility_str(e.reversibility),
                    e.inverse.as_ref().map(|c| c.action.clone()),
                    e.inverse
                        .as_ref()
                        .map(|c| serde_json::to_string(&c.payload))
                        .transpose()?,
                    e.timestamp,
                ],
            )
            .await?;
        }
        for r in kernel.all_relations() {
            tx.execute(
                "INSERT INTO relations (id, context_id, source, predicate, target, valid_from, valid_until, metadata_json) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    r.id.to_string(),
                    r.context.to_string(),
                    r.source.clone(),
                    r.predicate.clone(),
                    r.target.clone(),
                    r.valid_from,
                    r.valid_until,
                    serde_json::to_string(&r.metadata)?,
                ],
            )
            .await?;
        }
        for ev in kernel.events() {
            tx.execute(
                "INSERT INTO events (context_id, fiber_id, type, payload_json, timestamp) VALUES (?1,?2,?3,?4,?5)",
                params![
                    ev.context.map(|c| c.to_string()),
                    ev.fiber.map(|f| f.to_string()),
                    ev.ty.clone(),
                    serde_json::to_string(&ev.payload)?,
                    ev.timestamp,
                ],
            )
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    // ── Temporal World 查询（Phase 4：as_of / diff / replay 的数据源） ──

    /// 全部事件日志（append-only，按 id 升序；diff/replay 基础）。
    pub async fn event_log(&self) -> Result<Vec<StoredEvent>, PersistError> {
        self.query_events("SELECT context_id, fiber_id, type, payload_json, timestamp FROM events ORDER BY id ASC")
            .await
    }

    /// 时间区间事件（(t1, t2]；World::diff）。
    pub async fn events_between(&self, t1: i64, t2: i64) -> Result<Vec<StoredEvent>, PersistError> {
        let mut rows = self
            .conn
            .query(
                "SELECT context_id, fiber_id, type, payload_json, timestamp FROM events                  WHERE timestamp > ?1 AND timestamp <= ?2 ORDER BY id ASC",
                params![t1, t2],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().await? {
            out.push(row_to_event(&r)?);
        }
        Ok(out)
    }

    /// 单 fiber 事件序列（World::replay）。
    pub async fn events_of_fiber(&self, fiber: &str) -> Result<Vec<StoredEvent>, PersistError> {
        let mut rows = self
            .conn
            .query(
                "SELECT context_id, fiber_id, type, payload_json, timestamp FROM events                  WHERE fiber_id = ?1 ORDER BY id ASC",
                params![fiber],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().await? {
            out.push(row_to_event(&r)?);
        }
        Ok(out)
    }

    /// t 时刻有效的关系（valid 时间窗过滤；World::as_of）。
    pub async fn relations_valid_at(&self, t: i64) -> Result<Vec<StoredRelation>, PersistError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, context_id, source, predicate, target, valid_from, valid_until FROM relations                  WHERE valid_from <= ?1 AND (valid_until IS NULL OR valid_until > ?1) ORDER BY id ASC",
                params![t],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().await? {
            out.push(StoredRelation {
                id: r.get(0)?,
                context: r.get(1)?,
                source: r.get(2)?,
                predicate: r.get(3)?,
                target: r.get(4)?,
                valid_from: r.get(5)?,
                valid_until: r.get(6)?,
            });
        }
        Ok(out)
    }

    /// t 时刻有效的 Context（valid 时间窗过滤；World::as_of）。
    pub async fn contexts_valid_at(&self, t: i64) -> Result<Vec<StoredContext>, PersistError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, parent_id, entity_id, scope, state, valid_from, valid_until FROM contexts                  WHERE valid_from <= ?1 AND (valid_until IS NULL OR valid_until > ?1) ORDER BY id ASC",
                params![t],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().await? {
            out.push(StoredContext {
                id: r.get(0)?,
                parent: r.get(1)?,
                entity_id: r.get(2)?,
                scope: r.get(3)?,
                state: r.get(4)?,
                valid_from: r.get(5)?,
                valid_until: r.get(6)?,
            });
        }
        Ok(out)
    }

    async fn query_events(&self, sql: &str) -> Result<Vec<StoredEvent>, PersistError> {
        let mut rows = self.conn.query(sql, params![]).await?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().await? {
            out.push(row_to_event(&r)?);
        }
        Ok(out)
    }

    /// 从快照恢复 Kernel（id 计数器取 max+1）。
    pub async fn restore(&self) -> Result<Kernel, PersistError> {
        let mut k = Kernel::new();

        let mut rows = self
            .conn
            .query("SELECT id, parent_id, entity_id, scope, state, valid_from, valid_until, state_json FROM contexts", params![])
            .await?;
        while let Some(r) = rows.next().await? {
            let id = parse_id::<ContextId>(&r.get::<String>(0)?)?;
            let parent = opt_id::<ContextId>(r.get::<Option<String>>(1)?)?;
            let mut ctx = Context::new(
                id,
                parent,
                parse_scope(&r.get::<String>(3)?)?,
                r.get::<String>(2)?,
                r.get::<i64>(5)?,
            );
            ctx.valid_until = r.get::<Option<i64>>(6)?;
            ctx.state = parse_context_state(&r.get::<String>(4)?)?;
            let st: Value = serde_json::from_str(&r.get::<String>(7)?)?;
            ctx.state_store = super::state::StateStore::from_snapshot(st);
            k.restore_context(ctx);
        }

        let mut rows = self
            .conn
            .query("SELECT id, context_id, parent_id, state, task, input_json, output_json, wait_source, wait_predicate, wait_target, created_at FROM fibers", params![])
            .await?;
        while let Some(r) = rows.next().await? {
            let id = parse_id::<FiberId>(&r.get::<String>(0)?)?;
            let mut f = Fiber::new(
                id,
                parse_id::<ContextId>(&r.get::<String>(1)?)?,
                opt_id::<FiberId>(r.get::<Option<String>>(2)?)?,
                &r.get::<String>(4)?,
                serde_json::from_str(&r.get::<String>(5)?)?,
                r.get::<i64>(10)?,
            );
            f.state = parse_fiber_state(&r.get::<String>(3)?)?;
            f.output = r
                .get::<Option<String>>(6)?
                .map(|s| serde_json::from_str(&s))
                .transpose()?;
            let wp = r.get::<Option<String>>(8)?;
            if let Some(pred) = r.get::<Option<String>>(9)? {
                f.wait_for = Some(WaitPattern {
                    source: wp,
                    predicate: pred,
                    target: r.get::<Option<String>>(10)?,
                });
            }
            k.restore_fiber(f);
        }

        let mut rows = self
            .conn
            .query("SELECT id, fiber_id, action, input_json, output_json, status, reversibility, inverse_action, inverse_json, timestamp FROM effects", params![])
            .await?;
        while let Some(r) = rows.next().await? {
            let inverse_action = r.get::<Option<String>>(7)?;
            let inverse_payload = r
                .get::<Option<String>>(8)?
                .map(|s| serde_json::from_str(&s))
                .transpose()?
                .unwrap_or(Value::Null);
            let mut e = Effect::new(
                r.get::<String>(2)?,
                serde_json::from_str(&r.get::<String>(3)?)?,
                parse_reversibility(&r.get::<String>(6)?)?,
                inverse_action.map(|a| Compensation {
                    action: a,
                    payload: inverse_payload,
                }),
                r.get::<i64>(9)?,
            );
            e.id = parse_id::<EffectId>(&r.get::<String>(0)?)?;
            e.fiber = parse_id::<FiberId>(&r.get::<String>(1)?)?;
            e.output = r
                .get::<Option<String>>(4)?
                .map(|s| serde_json::from_str(&s))
                .transpose()?;
            e.status = parse_effect_state(&r.get::<String>(5)?)?;
            k.restore_effect(e);
        }

        let mut rows = self
            .conn
            .query("SELECT id, context_id, source, predicate, target, valid_from, valid_until, metadata_json FROM relations", params![])
            .await?;
        while let Some(r) = rows.next().await? {
            let mut rel = Relation::new(
                parse_id::<ContextId>(&r.get::<String>(1)?)?,
                r.get::<String>(2)?,
                r.get::<String>(3)?,
                r.get::<String>(4)?,
                r.get::<i64>(5)?,
            );
            rel.id = parse_id::<RelationId>(&r.get::<String>(0)?)?;
            rel.valid_until = r.get::<Option<i64>>(6)?;
            rel.metadata = serde_json::from_str(&r.get::<String>(7)?)?;
            k.restore_relation(rel);
        }

        let mut rows = self
            .conn
            .query("SELECT context_id, fiber_id, type, payload_json, timestamp FROM events ORDER BY id ASC", params![])
            .await?;
        while let Some(r) = rows.next().await? {
            k.restore_event(KernelEvent {
                context: opt_id::<ContextId>(r.get::<Option<String>>(0)?)?,
                fiber: opt_id::<FiberId>(r.get::<Option<String>>(1)?)?,
                ty: r.get::<String>(2)?,
                payload: serde_json::from_str(&r.get::<String>(3)?)?,
                timestamp: r.get::<i64>(4)?,
            });
        }

        Ok(k)
    }
}

// ── 序列化辅助 ─────────────────────────────────────────────────────────────

fn scope_str(s: Scope) -> &'static str {
    match s {
        Scope::World => "world",
        Scope::Tenant => "tenant",
        Scope::Session => "session",
        Scope::Agent => "agent",
        Scope::Task => "task",
        Scope::Action => "action",
    }
}

fn parse_scope(s: &str) -> Result<Scope, PersistError> {
    Ok(match s {
        "world" => Scope::World,
        "tenant" => Scope::Tenant,
        "session" => Scope::Session,
        "agent" => Scope::Agent,
        "task" => Scope::Task,
        "action" => Scope::Action,
        other => return Err(PersistError::Corrupt(format!("未知 scope: {other}"))),
    })
}

fn context_state_str(s: ContextState) -> &'static str {
    match s {
        ContextState::Active => "active",
        ContextState::Disposing => "disposing",
        ContextState::Disposed => "disposed",
    }
}

fn parse_context_state(s: &str) -> Result<ContextState, PersistError> {
    Ok(match s {
        "active" => ContextState::Active,
        "disposing" => ContextState::Disposing,
        "disposed" => ContextState::Disposed,
        other => {
            return Err(PersistError::Corrupt(format!(
                "未知 context state: {other}"
            )));
        }
    })
}

fn parse_fiber_state(s: &str) -> Result<FiberState, PersistError> {
    Ok(match s {
        "Created" => FiberState::Created,
        "Starting" => FiberState::Starting,
        "Ready" => FiberState::Ready,
        "Running" => FiberState::Running,
        "Waiting" => FiberState::Waiting,
        "Paused" => FiberState::Paused,
        "Completed" => FiberState::Completed,
        "Failed" => FiberState::Failed,
        "Compensating" => FiberState::Compensating,
        "Compensated" => FiberState::Compensated,
        "Escalated" => FiberState::Escalated,
        other => return Err(PersistError::Corrupt(format!("未知 fiber state: {other}"))),
    })
}

fn reversibility_str(r: Reversibility) -> &'static str {
    match r {
        Reversibility::Reversible => "Reversible",
        Reversibility::Compensatable => "Compensatable",
        Reversibility::Idempotent => "Idempotent",
        Reversibility::Irreversible => "Irreversible",
    }
}

fn parse_reversibility(s: &str) -> Result<Reversibility, PersistError> {
    Ok(match s {
        "Reversible" => Reversibility::Reversible,
        "Compensatable" => Reversibility::Compensatable,
        "Idempotent" => Reversibility::Idempotent,
        "Irreversible" => Reversibility::Irreversible,
        other => {
            return Err(PersistError::Corrupt(format!(
                "未知 reversibility: {other}"
            )));
        }
    })
}

fn effect_state_str(s: EffectState) -> &'static str {
    match s {
        EffectState::Committed => "Committed",
        EffectState::Compensated => "Compensated",
        EffectState::CompensationFailed => "CompensationFailed",
    }
}

fn parse_effect_state(s: &str) -> Result<EffectState, PersistError> {
    Ok(match s {
        "Committed" => EffectState::Committed,
        "Compensated" => EffectState::Compensated,
        "CompensationFailed" => EffectState::CompensationFailed,
        other => return Err(PersistError::Corrupt(format!("未知 effect state: {other}"))),
    })
}

fn parse_id<T: IdCodec>(s: &str) -> Result<T, PersistError> {
    T::parse(s).ok_or_else(|| PersistError::Corrupt(format!("非法 id: {s}")))
}

fn opt_id<T: IdCodec>(s: Option<String>) -> Result<Option<T>, PersistError> {
    s.map(|v| parse_id(&v)).transpose()
}

/// id 编解码（"ctx_1" → ContextId(1)）。
pub(crate) trait IdCodec {
    fn parse(s: &str) -> Option<Self>
    where
        Self: Sized;
}

macro_rules! impl_id_codec {
    ($t:ty, $prefix:literal) => {
        impl IdCodec for $t {
            fn parse(s: &str) -> Option<Self> {
                s.strip_prefix($prefix)
                    .and_then(|n| n.parse().ok())
                    .map(Self)
            }
        }
    };
}

impl_id_codec!(ContextId, "ctx_");
impl_id_codec!(FiberId, "fiber_");
impl_id_codec!(EffectId, "fx_");
impl_id_codec!(RelationId, "rel_");

// ── Temporal World 存储视图（Phase 4） ─────────────────────────────────────

/// 事件日志行（append-only）。
#[derive(Debug, Clone)]
pub struct StoredEvent {
    pub context: Option<String>,
    pub fiber: Option<String>,
    pub ty: String,
    pub payload: Value,
    pub timestamp: i64,
}

/// 关系行（含时间窗）。
#[derive(Debug, Clone)]
pub struct StoredRelation {
    pub id: String,
    pub context: String,
    pub source: String,
    pub predicate: String,
    pub target: String,
    pub valid_from: i64,
    pub valid_until: Option<i64>,
}

/// Context 行（含时间窗）。
#[derive(Debug, Clone)]
pub struct StoredContext {
    pub id: String,
    pub parent: Option<String>,
    pub entity_id: String,
    pub scope: String,
    pub state: String,
    pub valid_from: i64,
    pub valid_until: Option<i64>,
}

fn row_to_event(r: &libsql::Row) -> Result<StoredEvent, PersistError> {
    Ok(StoredEvent {
        context: r.get(0)?,
        fiber: r.get(1)?,
        ty: r.get(2)?,
        payload: serde_json::from_str(&r.get::<String>(3)?)?,
        timestamp: r.get(4)?,
    })
}
