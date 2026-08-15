//! WorkerRouter —— 候选 → Policy Filter → Score → Selected。

use super::policy::{WorkerPolicy, WorkerVerdict};
use super::registry::WorkerRegistry;
use super::sdk::WorkerError;

/// 路由判据。
#[derive(Debug, Clone)]
pub struct RouterCriteria {
    /// 显式指定 worker（跳过评分）。
    pub explicit: Option<String>,
    pub min_quality: f64,
    pub max_cost: u32,
}

impl Default for RouterCriteria {
    fn default() -> Self {
        Self {
            explicit: None,
            min_quality: 0.0,
            max_cost: u32::MAX,
        }
    }
}

/// 选中结果（可解释）。
#[derive(Debug, Clone)]
pub struct SelectedWorker {
    pub worker_id: String,
    pub capability: String,
    pub reason: String,
}

/// Worker Router：Capability + Policy + Quality/Cost/Latency → Worker。
pub struct WorkerRouter {
    pub criteria: RouterCriteria,
}

impl WorkerRouter {
    pub fn new(criteria: RouterCriteria) -> Self {
        Self { criteria }
    }

    pub fn route(
        &self,
        registry: &WorkerRegistry,
        capability: &str,
        policy: &WorkerPolicy,
    ) -> Result<SelectedWorker, WorkerError> {
        // 1) Policy Filter（fail-closed）
        match policy.check(capability, &serde_json::json!({})) {
            WorkerVerdict::Deny(reason) => return Err(WorkerError::Denied(reason)),
            WorkerVerdict::RequireApproval => {
                return Err(WorkerError::RequireApproval(capability.to_string()));
            }
            WorkerVerdict::Allow => {}
        }
        // 2) 候选
        let candidates = registry.candidates(capability);
        if candidates.is_empty() {
            return Err(WorkerError::NoWorker(capability.to_string()));
        }
        // 3) 显式指定
        if let Some(id) = &self.criteria.explicit {
            if let Some(w) = candidates.iter().find(|w| &w.manifest().id == id) {
                return Ok(SelectedWorker {
                    worker_id: w.manifest().id.clone(),
                    capability: capability.to_string(),
                    reason: "explicit".into(),
                });
            }
            return Err(WorkerError::NoWorker(format!(
                "{capability} 无显式 worker {id}"
            )));
        }
        // 4) 评分：quality 降序（同分 cost 升序、latency 升序）
        let best = candidates
            .iter()
            .filter(|w| {
                w.manifest().quality >= self.criteria.min_quality
                    && w.manifest().cost <= self.criteria.max_cost
            })
            .max_by(|a, b| {
                a.manifest()
                    .quality
                    .partial_cmp(&b.manifest().quality)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| b.manifest().cost.cmp(&a.manifest().cost))
                    .then_with(|| b.manifest().latency_ms.cmp(&a.manifest().latency_ms))
            })
            .ok_or_else(|| WorkerError::NoWorker(format!("{capability} 无满足判据的 worker")))?;
        Ok(SelectedWorker {
            worker_id: best.manifest().id.clone(),
            capability: capability.to_string(),
            reason: format!("highest quality (q={})", best.manifest().quality),
        })
    }
}
