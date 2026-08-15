//! ConditionExpr —— 结构化决策条件（Evolution ABI）。
//!
//! 闭包可以执行，但不能演化；ConditionExpr 可以
//! serialize → inspect → compare → validate → propose → mutate → audit → rollback。

use serde::{Deserialize, Serialize};

use crate::cognitive::engine::AgentView;

/// 结构化决策条件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ConditionExpr {
    /// 世界存在关系（source 任意）：`?--predicate-->target`。
    HasRelation { predicate: String, target: String },
    /// 认知层存在该 action 的风险信号（Phase 6 FailurePattern → RiskSignal）。
    HasRisk { action: String },
    /// 逻辑非。
    Not(Box<ConditionExpr>),
    /// 全部满足。
    All(Vec<ConditionExpr>),
    /// 任一满足。
    Any(Vec<ConditionExpr>),
    // HasState{..} —— future 扩展位（Phase 7 不实现；保持 Non-goal）
}

impl ConditionExpr {
    /// 在观察视图上求值（决策输入，只读）。
    pub fn matches(&self, view: &AgentView) -> bool {
        match self {
            ConditionExpr::HasRelation { predicate, target } => {
                view.has_relation(predicate, target)
            }
            ConditionExpr::HasRisk { action } => view.evolution.iter().any(|r| r.action == *action),
            ConditionExpr::Not(inner) => !inner.matches(view),
            ConditionExpr::All(items) => items.iter().all(|c| c.matches(view)),
            ConditionExpr::Any(items) => items.iter().any(|c| c.matches(view)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cognitive::engine::AgentView;
    use crate::evolution2::{EvolutionEngine, OutcomeKind};
    use crate::kernel::{Kernel, Scope};
    use serde_json::json;

    fn view_with_relation(k: &Kernel, ctx: crate::kernel::ContextId) -> AgentView {
        crate::cognitive::observe::observe(
            k,
            ctx,
            "goal",
            &crate::cognitive::Memory::new(),
            &[],
            None,
        )
    }

    #[test]
    fn condition_expr_evaluates_structurally() {
        let mut k = Kernel::new();
        let ctx = k.create_context(None, Scope::World, "w").unwrap();
        let t0 = k.now();
        k.relate(crate::kernel::Relation::new(
            ctx, "a", "status", "active", t0,
        ))
        .unwrap();
        let view = view_with_relation(&k, ctx);

        assert!(
            ConditionExpr::HasRelation {
                predicate: "status".into(),
                target: "active".into(),
            }
            .matches(&view)
        );
        assert!(
            !ConditionExpr::HasRelation {
                predicate: "status".into(),
                target: "banned".into(),
            }
            .matches(&view)
        );
        // Not / All / Any
        assert!(
            ConditionExpr::Not(Box::new(ConditionExpr::HasRelation {
                predicate: "status".into(),
                target: "banned".into(),
            }))
            .matches(&view)
        );
        assert!(
            !ConditionExpr::All(vec![
                ConditionExpr::HasRelation {
                    predicate: "status".into(),
                    target: "active".into(),
                },
                ConditionExpr::HasRelation {
                    predicate: "missing".into(),
                    target: "x".into(),
                }
            ])
            .matches(&view)
        );
        assert!(
            ConditionExpr::Any(vec![
                ConditionExpr::HasRelation {
                    predicate: "status".into(),
                    target: "active".into(),
                },
                ConditionExpr::HasRelation {
                    predicate: "missing".into(),
                    target: "x".into(),
                }
            ])
            .matches(&view)
        );
    }

    #[test]
    fn condition_expr_serializes_roundtrip() {
        let expr = ConditionExpr::All(vec![
            ConditionExpr::HasRelation {
                predicate: "status".into(),
                target: "active".into(),
            },
            ConditionExpr::Not(Box::new(ConditionExpr::HasRisk {
                action: "RefundCustomer".into(),
            })),
        ]);
        let s = serde_json::to_string(&expr).unwrap();
        let back: ConditionExpr = serde_json::from_str(&s).unwrap();
        assert_eq!(expr, back);
    }

    #[test]
    fn has_risk_reads_agent_view_evolution() {
        let mut k = Kernel::new();
        let ctx = k.create_context(None, Scope::World, "w").unwrap();
        let mut ev = EvolutionEngine::new();
        ev.record("RefundCustomer", OutcomeKind::Failed);
        ev.record("RefundCustomer", OutcomeKind::Failed);
        ev.mine();
        let view = crate::cognitive::observe::observe(
            &k,
            ctx,
            "goal",
            &crate::cognitive::Memory::new(),
            &[],
            Some(&ev),
        );
        assert!(
            ConditionExpr::HasRisk {
                action: "RefundCustomer".into()
            }
            .matches(&view)
        );
        let _ = json!({});
    }
}
