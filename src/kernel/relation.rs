//! Relation —— 动态图（Kernel Algebra ④：谁和谁发生了什么关系）。
//!
//! P3 Temporal Minimalism：Kernel 只保证 `is_current(now)`；
//! as_of / replay / bitemporal 属 Phase 4（Temporal World Model）。

use serde_json::Value;

use super::ContextId;

/// 关系边（带时间窗）。
#[derive(Debug, Clone)]
pub struct Relation {
    pub id: RelationId,
    pub context: ContextId,
    pub source: String,
    pub predicate: String,
    pub target: String,
    pub valid_from: i64,
    pub valid_until: Option<i64>,
    pub metadata: Value,
}

impl Relation {
    pub fn new(
        context: ContextId,
        source: impl Into<String>,
        predicate: impl Into<String>,
        target: impl Into<String>,
        now: i64,
    ) -> Self {
        Self {
            id: RelationId(0), // 由 Kernel::relate 分配
            context,
            source: source.into(),
            predicate: predicate.into(),
            target: target.into(),
            valid_from: now,
            valid_until: None,
            metadata: Value::Null,
        }
    }

    /// 当前有效（P3：Kernel 唯一的时间保证）。
    pub fn is_current(&self, now: i64) -> bool {
        now >= self.valid_from && self.valid_until.map(|u| now < u).unwrap_or(true)
    }

    /// 时间窗过滤（软过滤；as_of 精确重建属 Phase 4）。
    pub fn valid_at(&self, t: i64) -> bool {
        t >= self.valid_from && self.valid_until.map(|u| t < u).unwrap_or(true)
    }

    /// 结束一条关系（逻辑删除：valid_until = now）。
    pub fn expire(&mut self, now: i64) {
        self.valid_until = Some(now);
    }
}

/// Relation id。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RelationId(pub u64);

impl std::fmt::Display for RelationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "rel_{}", self.0)
    }
}
