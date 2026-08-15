//! PROJECTION —— Simulation 原语（V2 Kernel Invariant I2 的载体）。
//!
//! 双轨模型的「假设轨」：`Hypothetical Action → Projection → Predicted State`。
//!
//! 铁律（I2）：**Projection ∉ EventLog**——投影永不写入事件流、永不参与
//! `State = Reduce(EventLog)`、只能作为 Decision 的输入。类型层面保证：
//! Projection 没有到 SessionEvent / TraceEvent 的转换路径（无转换 API），
//! `SessionLog::append` / `Kernel::emit` 均不接受本类型。
//!
//! 可审计：basis_events（依据的真实事件）与 assumptions（假设清单）随投影保存，
//! 回答「当时依据什么事实、基于什么假设做了这个预测」。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::evaluation::EventRef;

/// 假设轨投影（hypothetical 恒为 true——类型即标记，杜绝假设混入事实）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Projection {
    pub id: String,
    /// 依据的真实事件（可审计：预测建立在哪些事实上）。
    pub basis_events: Vec<EventRef>,
    /// 假设清单（人可读；与事实区分）。
    pub assumptions: Vec<String>,
    /// 被模拟的假设动作（原语语义：Action；但本原语从不执行它）。
    pub hypothetical_action: Value,
    /// 预测状态（Reduce 语义的假设版本；永不落事实轨）。
    pub predicted_state: Value,
}

impl Projection {
    pub fn new(
        id: impl Into<String>,
        basis_events: Vec<EventRef>,
        assumptions: Vec<String>,
        hypothetical_action: Value,
        predicted_state: Value,
    ) -> Self {
        Self {
            id: id.into(),
            basis_events,
            assumptions,
            hypothetical_action,
            predicted_state,
        }
    }

    /// 类型即标记：投影永远是假设（I2）。
    pub fn hypothetical(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev_ref(seq: u64) -> EventRef {
        EventRef {
            seq,
            event_type: "relation.created".into(),
        }
    }

    #[test]
    fn projection_is_always_hypothetical() {
        let p = Projection::new(
            "proj_1",
            vec![ev_ref(0), ev_ref(1)],
            vec!["假设：派给 Worker A".into()],
            json!({ "action": "DeployProduction" }),
            json!({ "version": "V2" }),
        );
        assert!(p.hypothetical(), "类型即标记：投影永远是假设");
        assert_eq!(p.basis_events.len(), 2, "预测可审计：依据哪些事实");
        assert!(!p.assumptions.is_empty(), "假设清单必须显式");
    }
}
