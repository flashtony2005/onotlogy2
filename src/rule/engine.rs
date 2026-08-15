//! RuleEngine —— 结构化决策引擎（D2：implements CognitiveEngine，无特殊分支）。
//!
//! 严格保持：`AgentLoop → CognitiveEngine::decide() → RuleEngine → Decision
//! → TaskExecutor`。行为演化改变的是「Decision 如何产生」，不是「如何执行」。

use crate::cognitive::engine::{AgentView, CognitiveEngine, CognitiveError, Decision};

use super::BehaviorRule;

/// 规则决策引擎：顺序扫描 enabled 规则（priority 降序），条件满足 → Decision。
pub struct RuleEngine {
    rules: Vec<BehaviorRule>,
}

impl RuleEngine {
    pub fn new(rules: Vec<BehaviorRule>) -> Self {
        Self { rules }
    }

    pub fn from_store(store: &super::RuleStore) -> Self {
        Self {
            rules: store.rules.clone(),
        }
    }

    pub fn rules(&self) -> &[BehaviorRule] {
        &self.rules
    }

    pub fn rule(&self, id: &str) -> Option<&BehaviorRule> {
        self.rules.iter().find(|r| r.id == id)
    }
}

impl CognitiveEngine for RuleEngine {
    fn decide(&self, view: &AgentView) -> Result<Option<Decision>, CognitiveError> {
        let mut candidates: Vec<&BehaviorRule> = self.rules.iter().filter(|r| r.enabled).collect();
        candidates.sort_by_key(|r| std::cmp::Reverse(r.priority));
        for rule in candidates {
            if rule.condition.matches(view) {
                return Ok(Some(Decision {
                    capability: rule.capability.clone(),
                    args: rule.args.clone(),
                    confidence: 0.9,
                    rule: Some(rule.id.clone()),
                }));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cognitive::engine::FakeEngine;
    use crate::kernel::{Kernel, Scope};
    use crate::rule::{ConditionExpr, RuleStore};
    use serde_json::json;

    #[test]
    fn rule_engine_selects_by_priority_and_enabled() {
        let store = RuleStore::new()
            .with_rule(
                crate::rule::BehaviorRule::new(
                    "r-low",
                    "customer.retain",
                    json!({ "target": "cust-01" }),
                    ConditionExpr::HasRelation {
                        predicate: "status".into(),
                        target: "active".into(),
                    },
                )
                .with_priority(10),
            )
            .with_rule(
                crate::rule::BehaviorRule::new(
                    "r-high",
                    "customer.refund",
                    json!({ "target": "cust-01", "amount": 100 }),
                    ConditionExpr::HasRelation {
                        predicate: "status".into(),
                        target: "active".into(),
                    },
                )
                .with_priority(20),
            )
            .with_rule(
                crate::rule::BehaviorRule::new(
                    "r-disabled",
                    "customer.retain",
                    json!({ "target": "cust-02" }),
                    ConditionExpr::All(vec![]),
                )
                .with_priority(100),
            );
        // 停用 r-disabled
        let mut store = store;
        store.rules[2].enabled = false;

        let engine = RuleEngine::from_store(&store);
        let mut k = Kernel::new();
        let ctx = k.create_context(None, Scope::World, "w").unwrap();
        let t0 = k.now();
        k.relate(crate::kernel::Relation::new(
            ctx, "cust-01", "status", "active", t0,
        ))
        .unwrap();
        let view = crate::cognitive::observe::observe(
            &k,
            ctx,
            "goal",
            &crate::cognitive::Memory::new(),
            &["customer.retain".into(), "customer.refund".into()],
            None,
        );

        let d = engine.decide(&view).unwrap().expect("决策");
        assert_eq!(d.capability, "customer.refund", "priority 高者优先");
        assert_eq!(d.rule.as_deref(), Some("r-high"), "决策携带规则 id");
        // 停用规则不参与
        assert_ne!(d.rule.as_deref(), Some("r-disabled"));
        let _ = FakeEngine::new(vec![]);
    }

    #[test]
    fn rule_engine_no_decision_when_no_condition_matches() {
        let store = RuleStore::new().with_rule(crate::rule::BehaviorRule::new(
            "r1",
            "customer.retain",
            json!({}),
            ConditionExpr::HasRelation {
                predicate: "status".into(),
                target: "banned".into(),
            },
        ));
        let engine = RuleEngine::from_store(&store);
        let mut k = Kernel::new();
        let ctx = k.create_context(None, Scope::World, "w").unwrap();
        let view = crate::cognitive::observe::observe(
            &k,
            ctx,
            "goal",
            &crate::cognitive::Memory::new(),
            &[],
            None,
        );
        assert!(engine.decide(&view).unwrap().is_none());
    }
}
