//! V2.0 组合层（Phase 2）集成契约：Service / Capability / Policy / Task。
//!
//! - C1 删除用户闭环：Service 能力 → Task 声明 → Policy(require_approval) → WAITING
//!   → explicit approve 恢复 → Effect 提交 + Relation 投影 → COMPLETED
//! - C2 前置条件 Deny：业务拒绝（Effect 未提交）→ FAILED → 空补偿 → COMPENSATED + 事件
//! - C3 失败补偿：能力声明的 inverse 自动接入补偿链（逆序）
//! - C4 Task 声明约束（allowed capabilities）
//! - C5 多能力 Service 查找

use openaip_os::compose::policy::{Cmp, Policy, Precondition};
use openaip_os::compose::{Capability, Service, StepOutcome, Task, TaskExecutor};
use openaip_os::kernel::compensation::RecordingCompensator;
use openaip_os::kernel::effect::{EffectState, Reversibility};
use openaip_os::kernel::fiber::FiberState;
use openaip_os::kernel::{Kernel, Scope};
use serde_json::{Value, json};
use std::sync::Arc;

/// 短暂借用 helper：避免 TaskExecutor 长借用阻塞 kernel 直操作。
fn spawn(
    k: &mut Kernel,
    ctx: openaip_os::kernel::ContextId,
    task: &Task,
    input: Value,
) -> openaip_os::kernel::FiberId {
    TaskExecutor::new(k).spawn(ctx, task, input).unwrap()
}

fn step(
    k: &mut Kernel,
    f: openaip_os::kernel::FiberId,
    cap: &Capability,
    args: Value,
) -> StepOutcome {
    TaskExecutor::new(k).step(f, cap, args).unwrap()
}

fn compensate(k: &mut Kernel, f: openaip_os::kernel::FiberId) {
    TaskExecutor::new(k).compensate(f).unwrap();
}

/// 管理服务：user.create（免审批）+ user.delete（需审批，可逆，可补偿）。
fn admin_service() -> Service {
    Service::new("admin-service")
        .with_capability(
            Capability::new("user.create", "CreateUser")
                .with_reversibility(Reversibility::Reversible)
                .with_inverse("DeleteUser"),
        )
        .with_capability(
            Capability::new("user.delete", "DeleteUser")
                .with_reversibility(Reversibility::Reversible)
                .with_inverse("RestoreUser")
                .with_policy(Policy {
                    require_approval: true,
                    preconditions: vec![Precondition {
                        param: "level".into(),
                        op: Cmp::Ne,
                        value: json!("PROTECTED"),
                    }],
                })
                .with_projection("deleted", "true"),
        )
}

// ── C1：删除用户闭环 — 审批挂起 → explicit approve → Effect + 投影 → 完成 ──
#[tokio::test]
async fn tc_c1_delete_user_full_loop() {
    let svc = admin_service();
    let mut k = Kernel::new();
    let world = k.create_context(None, Scope::World, "world").unwrap();

    let task = Task::new("delete-user", "删除用户 Alice").allowing(&["user.delete"]);
    let f = spawn(
        &mut k,
        world,
        &task,
        json!({ "target": "user-001", "level": "NORMAL" }),
    );
    assert_eq!(k.fiber(f).unwrap().state, FiberState::Running);

    // 第一步：Policy require_approval → WAITING
    let cap = svc.capability("user.delete").unwrap();
    let out = step(
        &mut k,
        f,
        cap,
        json!({ "target": "user-001", "level": "NORMAL" }),
    );
    assert_eq!(out, StepOutcome::Waiting);
    assert_eq!(k.fiber(f).unwrap().state, FiberState::Waiting);

    // 人工审批：显式 approve → 恢复 RUNNING
    k.approve(f, "admin").unwrap();
    assert_eq!(
        k.fiber(f).unwrap().state,
        FiberState::Running,
        "显式 approve 应恢复"
    );

    // 恢复后重新执行 → Allow → Effect 提交 + deleted 投影
    let out2 = step(
        &mut k,
        f,
        cap,
        json!({ "target": "user-001", "level": "NORMAL" }),
    );
    assert_eq!(out2, StepOutcome::Committed);
    assert!(
        k.committed_effects(f)
            .iter()
            .any(|e| e.action == "DeleteUser")
    );
    assert!(
        k.relations()
            .any(|r| r.predicate == "deleted" && r.source == "user-001")
    );

    // 完成
    k.advance(f, FiberState::Completed).unwrap();
    assert_eq!(k.fiber(f).unwrap().state, FiberState::Completed);
}

// ── C2：前置条件 Deny — 业务拒绝不提交 Effect，空补偿 → COMPENSATED ──────
#[tokio::test]
async fn tc_c2_precondition_deny_no_effect() {
    let svc = admin_service();
    let mut k = Kernel::new();
    let world = k.create_context(None, Scope::World, "world").unwrap();

    let f = spawn(&mut k, world, &Task::new("delete-user", "g"), json!({}));
    // PROTECTED 用户 → Deny（前置条件不满足）
    let cap = svc.capability("user.delete").unwrap();
    let out = step(
        &mut k,
        f,
        cap,
        json!({ "target": "user-001", "level": "PROTECTED" }),
    );
    assert!(matches!(out, StepOutcome::Denied(_)));
    // Effect 未提交（空补偿链）
    assert!(k.committed_effects(f).is_empty());
    assert!(k.events().iter().any(|e| e.ty == "policy.denied"));
    // Fiber FAILED → 补偿（空链）→ COMPENSATED
    compensate(&mut k, f);
    assert_eq!(k.fiber(f).unwrap().state, FiberState::Compensated);
}

// ── C3：失败补偿 — 能力声明的 inverse 自动接入（逆序） ────────────────────
#[tokio::test]
async fn tc_c3_failure_compensation_uses_capability_inverse() {
    let recorder = Arc::new(RecordingCompensator::new());
    let svc = admin_service();
    let mut k = Kernel::new().with_compensator(recorder.clone());
    let world = k.create_context(None, Scope::World, "world").unwrap();

    let f = spawn(&mut k, world, &Task::new("multi", "g"), json!({}));
    // 两次提交（user.create 有 inverse DeleteUser）
    let cap = svc.capability("user.create").unwrap();
    assert_eq!(
        step(&mut k, f, cap, json!({ "target": "u1" })),
        StepOutcome::Committed
    );
    assert_eq!(
        step(&mut k, f, cap, json!({ "target": "u2" })),
        StepOutcome::Committed
    );

    // 动作失败 → FAILED → 补偿（inverse 逆序：DeleteUser ×2）
    TaskExecutor::new(&mut k).fail(f, "exec failed").unwrap();
    compensate(&mut k, f);
    assert_eq!(k.fiber(f).unwrap().state, FiberState::Compensated);
    let calls: Vec<String> = recorder
        .calls
        .lock()
        .unwrap()
        .iter()
        .map(|(a, _)| a.clone())
        .collect();
    assert_eq!(
        calls,
        vec!["DeleteUser", "DeleteUser"],
        "逆序补偿能力声明的 inverse"
    );
    assert!(
        k.committed_effects(f)
            .iter()
            .all(|e| e.status == EffectState::Compensated)
    );
}

// ── C4：Task 声明约束（allowed capabilities） ─────────────────────────────
#[tokio::test]
async fn tc_c4_task_allowed_capabilities_declared() {
    let task = Task::new("delete-user", "删除用户").allowing(&["user.delete"]);
    assert!(
        task.allowed_capabilities
            .contains(&"user.delete".to_string())
    );
    assert!(
        !task
            .allowed_capabilities
            .contains(&"user.create".to_string())
    );
    // 声明检查：service 提供的能力应覆盖 task 声明
    let svc = admin_service();
    for cap in &task.allowed_capabilities {
        assert!(
            svc.capability(cap).is_some(),
            "task 声明的能力应由 service 提供"
        );
    }
}

// ── C5：多能力 Service 查找 + 无审批直通 ──────────────────────────────────
#[tokio::test]
async fn tc_c5_service_multi_capability_direct_allow() {
    let svc = admin_service();
    let mut k = Kernel::new();
    let world = k.create_context(None, Scope::World, "world").unwrap();

    let f = spawn(&mut k, world, &Task::new("create", "g"), json!({}));
    // user.create 无审批 → 直接 Committed
    let cap = svc.capability("user.create").unwrap();
    assert_eq!(
        step(&mut k, f, cap, json!({ "target": "u3" })),
        StepOutcome::Committed
    );
    // 未知能力 → service 返回 None
    assert!(svc.capability("user.promote").is_none());
    let _: Value = json!(null);
}
