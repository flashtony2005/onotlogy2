//! Subgraph —— 图遍历检索（Phase 5 · D3：GraphRAG = Graph Retrieval，无 embedding）。
//!
//! seed → 关系可达遍历（深度限制）→ 子图（实体 + 关系），注入 AgentView。

use crate::kernel::Kernel;

use super::entity::EntityView;

/// 子图（语义检索结果）。
#[derive(Debug, Clone)]
pub struct Subgraph {
    pub seed: String,
    pub entities: Vec<EntityView>,
    /// 子图内关系 (source, predicate, target)。
    pub relations: Vec<(String, String, String)>,
}

/// seed 可达子图（BFS，max_depth 跳）。
pub fn subgraph(
    kernel: &Kernel,
    ctx: crate::kernel::ContextId,
    seed: &str,
    max_depth: usize,
) -> Subgraph {
    let now = kernel.now();
    let rels: Vec<(String, String, String)> = kernel
        .relations()
        .filter(|r| r.context == ctx && r.is_current(now))
        .map(|r| (r.source.clone(), r.predicate.clone(), r.target.clone()))
        .collect();

    // BFS 收集可达节点
    let mut visited: Vec<String> = vec![seed.to_string()];
    let mut frontier: Vec<String> = vec![seed.to_string()];
    let mut depth = 0;
    while !frontier.is_empty() && depth < max_depth {
        let mut next: Vec<String> = Vec::new();
        for node in &frontier {
            for (s, _, t) in &rels {
                if s == node && !visited.contains(t) {
                    visited.push(t.clone());
                    next.push(t.clone());
                }
                if t == node && !visited.contains(s) {
                    visited.push(s.clone());
                    next.push(s.clone());
                }
            }
        }
        frontier = next;
        depth += 1;
    }

    // 子图关系（两端都在 visited 内）
    let sub_rels: Vec<(String, String, String)> = rels
        .into_iter()
        .filter(|(s, _, t)| visited.contains(s) && visited.contains(t))
        .collect();

    // 实体视图（出入边限定在子图内）
    let mut entities: Vec<EntityView> = Vec::new();
    for id in &visited {
        let outgoing: Vec<(String, String)> = sub_rels
            .iter()
            .filter(|(s, _, _)| s == id)
            .map(|(_, p, t)| (p.clone(), t.clone()))
            .collect();
        let incoming: Vec<(String, String)> = sub_rels
            .iter()
            .filter(|(_, _, t)| t == id)
            .map(|(s, p, _)| (s.clone(), p.clone()))
            .collect();
        entities.push(EntityView {
            id: id.clone(),
            entity_type: id
                .split(['-', '_', '.'])
                .next()
                .unwrap_or("entity")
                .to_string(),
            state: serde_json::Value::Null,
            outgoing,
            incoming,
        });
    }

    Subgraph {
        seed: seed.to_string(),
        entities,
        relations: sub_rels,
    }
}
