//! M1b · G6 契约测试：run 状态映射与 ID/持久化字符串（v2_parity）。
//!
//! 契约：
//! - RunStatus 语义枚举从 V1 planner 迁出（session/run.rs），消费面不感知。
//! - FiberState → RunStatus 全状态映射（13 变体）：唯一区分 Failed / Interrupted，
//!   Waiting/Paused 归 Running（审批挂起 = 运行中 + human_approval 标记）。
//! - LoopOutcome → (RunStatus, human_approval)：Waiting/Denied 需人工，Completed 完成。
//! - generate_run_id 唯一且带 run_ 前缀；from_db_str 未知 → Interrupted（安全兜底）。

use openaip_os::cognitive::LoopOutcome;
use openaip_os::kernel::FiberState;
use openaip_os::session::run::{RunStatus, fiber_to_run_status, generate_run_id};

// ── G6a：FiberState 全状态映射表（13 变体，无遗漏）────────────────────
#[test]
fn g6a_fiber_state_full_mapping_table() {
    use FiberState::*;
    let cases = [
        (Created, RunStatus::Running),
        (Starting, RunStatus::Running),
        (Ready, RunStatus::Running),
        (Running, RunStatus::Running),
        (Waiting, RunStatus::Running),
        (Paused, RunStatus::Running),
        (Completed, RunStatus::Completed),
        (Failed, RunStatus::Failed),
        (Compensating, RunStatus::Failed),
        (Compensated, RunStatus::Failed),
        (Escalated, RunStatus::Failed),
        (Cancelled, RunStatus::Failed),
        (Interrupted, RunStatus::Interrupted),
    ];
    for (f, expected) in cases {
        assert_eq!(fiber_to_run_status(f), expected, "{f:?} 映射错误");
    }
}

// ── G6b：LoopOutcome → (RunStatus, human_approval) ─────────────────────
#[test]
fn g6b_loop_outcome_maps_status_and_human_approval() {
    assert_eq!(
        LoopOutcome::Completed.to_run_status(),
        (RunStatus::Completed, false)
    );
    assert_eq!(
        LoopOutcome::Waiting.to_run_status(),
        (RunStatus::Running, true)
    );
    assert_eq!(
        LoopOutcome::Denied("前置条件".into()).to_run_status(),
        (RunStatus::Failed, true)
    );
    assert_eq!(
        LoopOutcome::Failed("无决策".into()).to_run_status(),
        (RunStatus::Failed, false)
    );
    assert_eq!(
        LoopOutcome::Continue.to_run_status(),
        (RunStatus::Running, false)
    );
}

// ── G6c：run id 唯一性/格式 + from_db_str 兜底 ────────────────────────
#[test]
fn g6c_run_id_and_db_str_roundtrip() {
    let a = generate_run_id();
    let b = generate_run_id();
    assert_ne!(a, b, "run id 应全局唯一");
    assert!(a.starts_with("run_"), "run id 前缀: {a}");

    for (s, expected) in [
        ("Running", RunStatus::Running),
        ("Completed", RunStatus::Completed),
        ("Failed", RunStatus::Failed),
    ] {
        assert_eq!(RunStatus::from_db_str(s), expected);
    }
    // 未知字符串 → Interrupted（安全兜底，不误判 Running；与 V1 语义一致）
    assert_eq!(RunStatus::from_db_str("Bogus"), RunStatus::Interrupted);
    // 往返：枚举 → 字符串 → 枚举
    for st in [
        RunStatus::Running,
        RunStatus::Completed,
        RunStatus::Failed,
        RunStatus::Interrupted,
    ] {
        assert_eq!(RunStatus::from_db_str(st.as_str()), st);
    }
}
