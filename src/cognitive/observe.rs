//! Observe —— 世界观察（决策输入构建）。
//!
//! 观察 = kernel fact 层（relations 当前有效投影）+ 记忆召回 + 可用能力。
//! 观察是只读的（不修改世界）；Observation 属于认知状态（P4）。

use serde_json::json;

use super::engine::AgentView;
use super::memory::Memory;
use crate::kernel::Kernel;

/// 构建观察视图（D5 输入：Agent 据此确认世界状态，而非相信自己执行过）。
/// Phase 5：观察 = World Model 投影（实体 + 子图 + 关系）→ AgentView。
pub fn observe(
    kernel: &Kernel,
    ctx: crate::kernel::ContextId,
    goal: &str,
    memory: &Memory,
    capabilities: &[String],
) -> AgentView {
    let now = kernel.now();
    let related: Vec<(String, String, String)> = kernel
        .relations()
        .filter(|r| r.context == ctx && r.is_current(now))
        .map(|r| (r.source.clone(), r.predicate.clone(), r.target.clone()))
        .collect();
    // 实体视图（relations 端点聚合）+ 种子子图（首个实体 2 跳）
    let entities = crate::world::derive_entities(kernel, ctx);
    let subgraph = related
        .first()
        .map(|(s, _, _)| crate::world::subgraph(kernel, ctx, s, 2));
    AgentView {
        goal: goal.to_string(),
        world: json!({ "relations": related, "entities": entities }),
        related,
        memory: memory.recall_all(),
        capabilities: capabilities.to_vec(),
        entities,
        subgraph,
    }
}
