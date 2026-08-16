//! M1b · G5 契约测试：失败数据源 = evolution2 分账（跨快照恢复生效）。
//!
//! 契约（V1 count_action_failed 语义迁移）：
//! - Gate 的「无失败」条件读 evolution2 账本（failed==0），非 trace 表计数。
//! - 失败历史经 snapshot/restore（CognitiveStore 持久化）跨 run 仍阻断 Gate。
//! - 纯成功历史经恢复后保持放行。

use openaip_os::evolution2::{EvolutionEngine, OutcomeKind};
use openaip_os::kernel::Reversibility;

// ── G5a：失败历史跨快照恢复仍阻断 Gate ───────────────────────────────
#[test]
fn g5a_failure_blocks_gate_after_snapshot_restore() {
    let mut ev = EvolutionEngine::new().with_gate_enabled(true);
    // 3 次成功满足 Success 必要条件 + 1 次失败 → Failure=0 不满足 → Wait
    for _ in 0..3 {
        ev.record("RefundCustomer", OutcomeKind::Success);
    }
    ev.record("RefundCustomer", OutcomeKind::Failed);
    ev.mine();
    ev.propose();
    ev.validate();
    assert!(
        !ev.gate("RefundCustomer", Reversibility::Compensatable),
        "有失败记录不得放行"
    );

    // 快照 → 恢复（模拟跨 run 重启：CognitiveStore save/load）
    let restored = EvolutionEngine::restore(ev.snapshot());
    assert!(
        !restored.gate("RefundCustomer", Reversibility::Compensatable),
        "失败历史跨重启仍阻断（数据源 = 分账，非 trace 计数）"
    );
}

// ── G5b：纯成功历史跨快照恢复保持放行 ────────────────────────────────
#[test]
fn g5b_success_only_restores_to_pass() {
    let mut ev = EvolutionEngine::new().with_gate_enabled(true);
    for _ in 0..5 {
        ev.record("RefundCustomer", OutcomeKind::Success);
    }
    ev.mine();
    ev.propose();
    ev.validate();
    assert!(ev.gate("RefundCustomer", Reversibility::Compensatable));

    let restored = EvolutionEngine::restore(ev.snapshot());
    assert!(
        restored.gate("RefundCustomer", Reversibility::Compensatable),
        "成功历史跨重启保持放行"
    );
}

// ── G5c：恢复后继续记录失败 → 放行立即收回 ───────────────────────────
#[test]
fn g5c_failure_after_restore_revokes_gate() {
    let mut ev = EvolutionEngine::new().with_gate_enabled(true);
    for _ in 0..5 {
        ev.record("RefundCustomer", OutcomeKind::Success);
    }
    ev.mine();
    ev.propose();
    ev.validate();
    let mut restored = EvolutionEngine::restore(ev.snapshot());
    assert!(restored.gate("RefundCustomer", Reversibility::Compensatable));

    restored.record("RefundCustomer", OutcomeKind::Failed);
    assert!(
        !restored.gate("RefundCustomer", Reversibility::Compensatable),
        "恢复后的失败记录应立即阻断（数据源一致性）"
    );
}
