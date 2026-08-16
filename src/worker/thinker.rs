//! Thinker（M1b · G8）—— V2 决策路径产出 Worker 任务。
//!
//! V1 桥（`--via-workers`）走 `Planner::plan`（V1 认知引擎 + ActionEngine 注册表）；
//! V2 桥走 `CognitiveEngine::decide` 循环的规划形态：纯规划、不执行、无世界副作用。
//!
//! 收敛保证（防环）：
//! - 引擎返回 `None` → 收敛（D5 语义）；
//! - 重复能力 → 收敛（规划不执行，无世界变化，须防「永远有决策」的引擎）；
//! - 决策数达能力数上限 → 收敛。
//!
//! 能力映射：compose 能力命名已是 `{object}.{action}` 点分形式，与 Worker 能力市场
//! 同名直映——V1 的 `PlanStep::worker_capability` 映射随 planner 一并删除，不再维护第二套映射。

use serde_json::json;

use super::orchestrator::WorkerTask;
use crate::cognitive::engine::{AgentView, CognitiveEngine, Decision};

/// V2 Thinker：认知引擎驱动的规划（决策序列 → Worker 任务）。
pub struct Thinker<'a> {
    engine: &'a dyn CognitiveEngine,
}

impl<'a> Thinker<'a> {
    pub fn new(engine: &'a dyn CognitiveEngine) -> Self {
        Self { engine }
    }

    /// 纯规划：goal + 可用能力 + 世界观察 → 决策序列（不执行、无副作用）。
    /// 防环收敛：None / 重复能力 / 能力数上限。
    pub fn plan(
        &self,
        goal: &str,
        capabilities: &[String],
        related: Vec<(String, String, String)>,
    ) -> Vec<Decision> {
        let view = AgentView {
            goal: goal.to_string(),
            world: json!({}),
            related,
            memory: vec![],
            capabilities: capabilities.to_vec(),
            entities: vec![],
            subgraph: None,
            evolution: vec![],
        };
        let mut out: Vec<Decision> = Vec::new();
        loop {
            match self.engine.decide(&view) {
                Ok(Some(d)) => {
                    if out.iter().any(|x| x.capability == d.capability)
                        || out.len() >= capabilities.len()
                    {
                        break; // 防环：重复能力或达到能力数上限 → 收敛
                    }
                    out.push(d);
                }
                Ok(None) => break,
                // 规划 best-effort：引擎错误在此终止（执行层另有 fail-closed）
                Err(_) => break,
            }
        }
        out
    }

    /// 决策 → WorkerTask（capability 同名直映；args 为 Worker input；note 可读）。
    pub fn to_tasks(decisions: &[Decision]) -> Vec<WorkerTask> {
        decisions
            .iter()
            .map(|d| WorkerTask {
                capability: d.capability.clone(),
                input: d.args.clone(),
                note: format!("决策执行能力 {}", d.capability),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 防环：永远返回同一能力的引擎必须收敛（能力数上限 + 重复去重）。
    #[test]
    fn plan_converges_on_repeat() {
        use crate::cognitive::engine::FakeEngine;
        let engine = FakeEngine::new(vec![crate::cognitive::engine::PlanStep {
            capability: "a.one".into(),
            args: json!({}),
            when: None,
        }]);
        let t = Thinker::new(&engine);
        let ds = t.plan("g", &["a.one".to_string()], vec![]);
        assert_eq!(ds.len(), 1);
    }
}
