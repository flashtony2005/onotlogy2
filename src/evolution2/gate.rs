//! EvolutionGate —— 审批替代闸门（Phase 6 · D4，V2-native 语义复刻）。
//!
//! V1 的语义可以复刻，V1 的代码/数据模型不复刻。门是 **AND Gate**：
//! `SuccessCount ≥ 3` 是必要条件，不是充分条件——绝不是 `success >= 3 → auto approve`。
//!
//! > **Gate failure means "cannot substitute approval", not "permission denied".**
//! > Gate 失败 = 不能替代人工审批 → 回退 Human Approval（WAIT），**不是拒绝执行**。

use crate::evolution2::EvolutionPermissionState;
use crate::kernel::Reversibility;

/// 验证阈值（置信度 ≥ 0.8）。
pub const VALIDATE_THRESHOLD: f64 = 0.8;
/// 置信度除数（`min(1, success / 5)`）。
pub const CONFIDENCE_DIVISOR: f64 = 5.0;
/// Gate 放行的最少成功次数（必要条件）。
pub const MIN_SUCCESS: u64 = 3;
/// SuccessPattern 提炼阈值。
pub const MIN_SUCCESS_PATTERN: u64 = 3;
/// FailurePattern（风险）提炼阈值。
pub const MIN_RISK_PATTERN: u64 = 2;

/// Gate 判定结果：`Pass` = 可替代人工审批；`Wait` = 回退人工审批（**非 DENY**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateVerdict {
    Pass,
    Wait,
}

/// 该可逆性是否满足 `Compensable ∧ ¬Irreversible`。
fn compensable(rev: Reversibility) -> bool {
    matches!(
        rev,
        Reversibility::Reversible | Reversibility::Compensatable
    )
}

/// 7 条件 AND 判定（纯函数，E5 直接可测）。
///
/// ```
/// EvolutionGate = Enabled ∧ Validated ∧ Confidence≥0.8 ∧ Success≥3
///               ∧ Failure=0 ∧ Compensable ∧ ¬Irreversible
/// ```
pub fn check(
    enabled: bool,
    state: EvolutionPermissionState,
    auto_approve_enabled: bool,
    confidence: f64,
    success: u64,
    failed: u64,
    reversibility: Reversibility,
) -> GateVerdict {
    let pass = enabled
        // Validated（Enabled 亦通过；Revoked/Disabled/Candidate 不通过）
        && matches!(state, EvolutionPermissionState::Validated | EvolutionPermissionState::Enabled)
        && auto_approve_enabled
        && confidence >= VALIDATE_THRESHOLD
        && success >= MIN_SUCCESS
        && failed == 0
        && compensable(reversibility);
    if pass {
        GateVerdict::Pass
    } else {
        GateVerdict::Wait
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evolution2::OutcomeKind;

    #[test]
    fn and_gate_all_conditions_required() {
        let st = EvolutionPermissionState::Validated;
        let base = (
            true,
            st,
            true,
            1.0,
            5u64,
            0u64,
            Reversibility::Compensatable,
        );
        // 全部满足 → Pass
        assert_eq!(
            check(base.0, base.1, base.2, base.3, base.4, base.5, base.6),
            GateVerdict::Pass
        );
        // 任一缺失 → Wait（不是 Deny）
        let cases: Vec<(
            bool,
            EvolutionPermissionState,
            bool,
            f64,
            u64,
            u64,
            Reversibility,
        )> = vec![
            (false, st, true, 1.0, 5, 0, Reversibility::Compensatable), // Enabled 缺失
            (
                true,
                EvolutionPermissionState::Disabled,
                true,
                1.0,
                5,
                0,
                Reversibility::Compensatable,
            ), // 未 Validated
            (
                true,
                EvolutionPermissionState::Revoked,
                false,
                1.0,
                5,
                0,
                Reversibility::Compensatable,
            ), // Revoked
            (true, st, false, 1.0, 5, 0, Reversibility::Compensatable), // auto_approve 关闭
            (true, st, true, 0.7, 5, 0, Reversibility::Compensatable),  // 置信 < 0.8
            (true, st, true, 1.0, 2, 0, Reversibility::Compensatable),  // 成功 < 3
            (true, st, true, 1.0, 5, 1, Reversibility::Compensatable),  // 有失败
            (true, st, true, 1.0, 5, 0, Reversibility::Idempotent),     // 不可补偿
            (true, st, true, 1.0, 5, 0, Reversibility::Irreversible),   // 不可逆
        ];
        for (i, c) in cases.iter().enumerate() {
            assert_eq!(
                check(c.0, c.1, c.2, c.3, c.4, c.5, c.6),
                GateVerdict::Wait,
                "case {i} 应 Wait（不能替代审批，而非权限拒绝）"
            );
        }
    }

    #[test]
    fn record_is_not_authority() {
        // 只有成功经验，无 proposal/validate → 不放行
        let mut ev = crate::evolution2::EvolutionEngine::new().with_gate_enabled(true);
        for _ in 0..5 {
            ev.record("RetainCustomer", OutcomeKind::Success);
        }
        ev.mine();
        assert!(!ev.gate("RetainCustomer", Reversibility::Compensatable));
    }
}
