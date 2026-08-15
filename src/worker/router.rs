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
    /// 最大风险等级（1-10；超限候选被过滤，Phase 11D）。
    pub max_risk: u32,
}

impl Default for RouterCriteria {
    fn default() -> Self {
        Self {
            explicit: None,
            min_quality: 0.0,
            max_cost: u32::MAX,
            max_risk: 10,
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

/// Worker Router：Capability + Policy + Quality/Cost/Latency/Risk/Trust → Worker。
#[derive(Clone)]
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
        // 4) 评分：quality × trust 降序（risk 超限过滤；同分 cost 升序、latency 升序）
        let best = candidates
            .iter()
            .filter(|w| {
                w.manifest().quality >= self.criteria.min_quality
                    && w.manifest().cost <= self.criteria.max_cost
                    && w.manifest().risk <= self.criteria.max_risk
            })
            .max_by(|a, b| {
                let sa = a.manifest().quality * a.manifest().trust;
                let sb = b.manifest().quality * b.manifest().trust;
                sa.partial_cmp(&sb)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| b.manifest().cost.cmp(&a.manifest().cost))
                    .then_with(|| b.manifest().latency_ms.cmp(&a.manifest().latency_ms))
            })
            .ok_or_else(|| WorkerError::NoWorker(format!("{capability} 无满足判据的 worker")))?;
        Ok(SelectedWorker {
            worker_id: best.manifest().id.clone(),
            capability: capability.to_string(),
            reason: format!(
                "highest score (q={} × t={})",
                best.manifest().quality,
                best.manifest().trust
            ),
        })
    }
}
