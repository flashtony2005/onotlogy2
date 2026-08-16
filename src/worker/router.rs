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
    /// Router Selection Learning（Trust 轴）：(capability, worker_id) → learned 修正（已封顶 ±0.1）。
    /// **只影响评分（Policy Filter 之后）**；Capability/Approval 永不学习（I4/I5）。
    /// 数据源：`worker::stats::learned_trust_map`（从 wb_worker_stats 派生）。
    pub learned_trust: std::collections::HashMap<(String, String), f64>,
}

impl Default for RouterCriteria {
    fn default() -> Self {
        Self {
            explicit: None,
            min_quality: 0.0,
            max_cost: u32::MAX,
            max_risk: 10,
            learned_trust: std::collections::HashMap::new(),
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
        // 4) 评分：quality × effective_trust 降序（risk 超限过滤；同分 cost 升序、latency 升序）
        //    effective_trust = declared trust + learned 修正（Trust 轴学习；Policy Filter 之后生效）
        let best = candidates
            .iter()
            .filter(|w| {
                w.manifest().quality >= self.criteria.min_quality
                    && w.manifest().cost <= self.criteria.max_cost
                    && w.manifest().risk <= self.criteria.max_risk
            })
            .max_by(|a, b| {
                let sa = a.manifest().quality * self.effective_trust(**a, capability);
                let sb = b.manifest().quality * self.effective_trust(**b, capability);
                sa.partial_cmp(&sb)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| b.manifest().cost.cmp(&a.manifest().cost))
                    .then_with(|| b.manifest().latency_ms.cmp(&a.manifest().latency_ms))
            })
            .ok_or_else(|| WorkerError::NoWorker(format!("{capability} 无满足判据的 worker")))?;
        let learned = self
            .criteria
            .learned_trust
            .get(&(capability.to_string(), best.manifest().id.clone()))
            .copied()
            .unwrap_or(0.0);
        Ok(SelectedWorker {
            worker_id: best.manifest().id.clone(),
            capability: capability.to_string(),
            reason: format!(
                "highest score (q={} × t={}{})",
                best.manifest().quality,
                best.manifest().trust,
                if learned != 0.0 {
                    format!(", learned {learned:+.2}")
                } else {
                    String::new()
                }
            ),
        })
    }

    /// effective trust = declared + learned 修正（clamp 0..1；修正已在 stats 层封顶 ±0.1）。
    fn effective_trust(&self, w: &dyn crate::worker::sdk::Worker, capability: &str) -> f64 {
        let learned = self
            .criteria
            .learned_trust
            .get(&(capability.to_string(), w.manifest().id.clone()))
            .copied()
            .unwrap_or(0.0);
        (w.manifest().trust + learned).clamp(0.0, 1.0)
    }
}
