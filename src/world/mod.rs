//! Temporal World —— 世界的时间投影（Phase 4；P3 边界：Kernel 只管当前有效，
//! 时间旅行查询属于 World Model 层）。
//!
//! - `as_of(t)`：重建 t 时刻的世界（relations/contexts 的 valid 时间窗）
//! - `diff(t1, t2)`：时间区间内的世界变化（事件日志 (t1, t2]）
//! - `replay(fiber)`：重放单个 fiber 的 Effect/关系序列（事件日志按 fiber）
//!
//! 数据源：kernel/persistence 的 append-only events 表 + relations/contexts 的
//! valid_from/valid_until 时间窗（save 保留历史行——expire 只闭合窗口不删行）。

use serde_json::Value;

pub mod entity;
pub mod subgraph;

pub use entity::{
    EntityData, EntityDataProvider, EntityView, NullEntityDataProvider, derive_entities,
};
pub use subgraph::{Subgraph, subgraph};

use crate::kernel::persistence::{
    PersistError, Persistence, StoredContext, StoredEvent, StoredRelation,
};

/// t 时刻的世界快照（as_of 结果）。
#[derive(Debug, Clone)]
pub struct WorldSnapshot {
    pub at: i64,
    /// 有效关系 (source, predicate, target)。
    pub relations: Vec<(String, String, String)>,
    /// 有效 Context（作用域存在性）。
    pub contexts: Vec<ContextInfo>,
}

/// Context 时间信息（as_of 视图）。
#[derive(Debug, Clone)]
pub struct ContextInfo {
    pub id: String,
    pub entity_id: String,
    pub scope: String,
    pub valid_from: i64,
    pub valid_until: Option<i64>,
}

/// 一次世界变化（diff/replay 的事件视图）。
#[derive(Debug, Clone)]
pub struct WorldChange {
    pub at: i64,
    pub ty: String,
    pub fiber: Option<String>,
    pub payload: Value,
}

/// Temporal World（只读查询；写路径仍在 Kernel）。
pub struct World<'p> {
    persistence: &'p Persistence,
}

impl<'p> World<'p> {
    pub fn new(persistence: &'p Persistence) -> Self {
        Self { persistence }
    }

    /// as_of(t)：重建 t 时刻的世界（relations/contexts 的 valid 时间窗过滤）。
    pub async fn as_of(&self, t: i64) -> Result<WorldSnapshot, PersistError> {
        let rels = self.persistence.relations_valid_at(t).await?;
        let ctxs = self.persistence.contexts_valid_at(t).await?;
        Ok(WorldSnapshot {
            at: t,
            relations: rels
                .into_iter()
                .map(|r| (r.source, r.predicate, r.target))
                .collect(),
            contexts: ctxs
                .into_iter()
                .map(|c: StoredContext| ContextInfo {
                    id: c.id,
                    entity_id: c.entity_id,
                    scope: c.scope,
                    valid_from: c.valid_from,
                    valid_until: c.valid_until,
                })
                .collect(),
        })
    }

    /// diff(t1, t2)：世界变化区间（事件日志 (t1, t2]）。
    pub async fn diff(&self, t1: i64, t2: i64) -> Result<Vec<WorldChange>, PersistError> {
        let events = self.persistence.events_between(t1, t2).await?;
        Ok(to_changes(events))
    }

    /// replay(fiber)：重放某 fiber 的全部事件序列。
    pub async fn replay(&self, fiber: &str) -> Result<Vec<WorldChange>, PersistError> {
        let events = self.persistence.events_of_fiber(fiber).await?;
        Ok(to_changes(events))
    }

    /// 全部事件日志（审计/调试）。
    pub async fn event_log(&self) -> Result<Vec<WorldChange>, PersistError> {
        let events = self.persistence.event_log().await?;
        Ok(to_changes(events))
    }
}

fn to_changes(events: Vec<StoredEvent>) -> Vec<WorldChange> {
    events
        .into_iter()
        .map(|e: StoredEvent| WorldChange {
            at: e.timestamp,
            ty: e.ty,
            fiber: e.fiber,
            payload: e.payload,
        })
        .collect()
}

/// StoredRelation 保持可见（供上层扩展）。
#[allow(dead_code)]
fn _rel_view(r: &StoredRelation) -> (String, String, String) {
    (r.source.clone(), r.predicate.clone(), r.target.clone())
}
