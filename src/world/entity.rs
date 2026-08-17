//! Entity —— World Model 的实体视图（Phase 5 · D2）。
//!
//! Entity 由 V2 fact graph（relations 端点聚合）推导，**不依赖 V1 storage**；
//! `EntityDataProvider` 是 V1 对象桥接的留口（默认 Null）。

use serde::Serialize;
use serde_json::Value;

use crate::kernel::Kernel;

/// 实体视图（Agent 认识世界的最小单元）。
#[derive(Debug, Clone, Serialize)]
pub struct EntityView {
    pub id: String,
    /// 类型（前缀推导，如 "user-001" → "user"；EntityDataProvider 可覆写）。
    pub entity_type: String,
    pub state: Value,
    /// 出边 (predicate, 类型化值)。值优先取 relation.metadata（保留数值类型），
    /// 否则回退 String(target)（兼容旧关系 / 纯字符串目标）。
    pub outgoing: Vec<(String, Value)>,
    /// 入边 (source 实体 id, predicate)。
    pub incoming: Vec<(String, Value)>,
}

/// 实体数据提供方（V1 对象桥接留口；Phase 5 默认不实现）。
pub trait EntityDataProvider: Send + Sync {
    /// 补充实体数据（type/state）；None = 由关系图推导。
    fn get(&self, id: &str) -> Option<EntityData>;
}

/// 补充数据（提供方返回）。
#[derive(Debug, Clone, Default)]
pub struct EntityData {
    pub entity_type: Option<String>,
    pub state: Value,
}

/// 空提供方（图推导兜底）。
pub struct NullEntityDataProvider;

impl EntityDataProvider for NullEntityDataProvider {
    fn get(&self, _id: &str) -> Option<EntityData> {
        None
    }
}

/// 由关系前缀推导类型（"user-001" → "user"）。
fn type_of(id: &str) -> String {
    id.split(['-', '_', '.'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("entity")
        .to_string()
}

/// 从 Kernel relations 聚合出实体视图集合（当前有效关系）。
pub fn derive_entities(kernel: &Kernel, ctx: crate::kernel::ContextId) -> Vec<EntityView> {
    let now = kernel.now();
    // (source, predicate, target, 类型化值)；类型化值优先取 relation.metadata（数值不文本化），
    // 否则回退 String(target)（兼容未带 metadata 的旧关系 / 纯字符串目标）。
    let rels: Vec<(String, String, String, Value)> = kernel
        .relations()
        .filter(|r| r.context == ctx && r.is_current(now))
        .map(|r| {
            let typed = if r.metadata.is_null() {
                Value::String(r.target.clone())
            } else {
                r.metadata.clone()
            };
            (
                r.source.clone(),
                r.predicate.clone(),
                r.target.clone(),
                typed,
            )
        })
        .collect();

    let mut ids: Vec<String> = Vec::new();
    for (s, _, t, _) in &rels {
        if !ids.contains(s) {
            ids.push(s.clone());
        }
        if !ids.contains(t) {
            ids.push(t.clone());
        }
    }
    ids.sort();

    ids.into_iter()
        .map(|id| {
            let outgoing: Vec<(String, Value)> = rels
                .iter()
                .filter(|(s, _, _, _)| s == &id)
                .map(|(_, p, _, v)| (p.clone(), v.clone()))
                .collect();
            let incoming: Vec<(String, Value)> = rels
                .iter()
                .filter(|(_, _, t, _)| t == &id)
                .map(|(s, p, _, _)| (s.clone(), Value::String(p.clone())))
                .collect();
            EntityView {
                entity_type: type_of(&id),
                state: Value::Null,
                outgoing,
                incoming,
                id,
            }
        })
        .collect()
}
