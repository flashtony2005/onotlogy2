//! V2.0 Runtime Kernel Phase 1 集成契约：
//! - 删除用户闭环（审批等待 → relate 恢复 → 完成）
//! - P1 补偿链（A✓B✓C✗ → 逆序补偿 → Compensated；失败 → Escalated + 事实）
//! - 持久化往返（save → restore）

mod common;

use openaip_os::kernel::compensation::RecordingCompensator;
use openaip_os::kernel::effect::{Effect, Reversibility};
use openaip_os::kernel::fiber::{FiberId, FiberState, WaitPattern};
use openaip_os::kernel::relation::Relation;
use openaip_os::kernel::{
    Compensation, CompensationEngine, ContextId, ContextState, Kernel, KernelError, Scope,
};
use serde_json::json;
use std::sync::Arc;

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

/// 造一个 RUNNING fiber。
fn running(k: &mut Kernel, ctx: ContextId, task: &str) -> FiberId {
    let f = k.spawn(ctx, None, task, json!({})).unwrap();
    k.advance(f, FiberState::Starting).unwrap();
    k.advance(f, FiberState::Ready).unwrap();
    k.advance(f, FiberState::Running).unwrap();
    f
}

/// 提交一个带补偿的 Effect。
fn emit(k: &mut Kernel, f: FiberId, action: &str, inverse: Option<&str>) {
    let comp = inverse.map(|a| Compensation {
        action: a.to_string(),
        payload: json!({ "of": action }),
    });
    let e = Effect::new(
        action,
        json!({ "action": action }),
        Reversibility::Compensatable,
        comp,
        now(),
    );
    k.emit(f, e).unwrap();
}

// ── TC-K1：删除用户闭环 — 审批挂起 → relate(Approval) 恢复 → 完成 ─────────
#[tokio::test]
async fn tc_k1_delete_user_approval_loop() {
    let mut k = Kernel::new();
    let world = k.create_context(None, Scope::World, "world").unwrap();
    let agent = k
        .create_context(Some(world), Scope::Agent, "admin-agent")
        .unwrap();

    // Task Fiber：删除用户（需人工审批）
    let f = running(&mut k, agent, "DeleteUser");
    // Policy 门禁：require_approval → 声明等待 Approval 关系，进入 WAITING
    k.wait_on(
        f,
        WaitPattern {
            source: None,
            predicate: "Approval".into(),
            target: None,
        },
    )
    .unwrap();
    k.advance(f, FiberState::Waiting).unwrap();
    assert_eq!(k.fiber(f).unwrap().state, FiberState::Waiting);

    // 浏览器/外部：POST /approval → relate(Approval --approved_by--> admin)
    k.relate(Relation::new(agent, "approval", "Approval", "admin", now()))
        .unwrap();
    // 等待匹配 → 自动恢复 RUNNING
    assert_eq!(
        k.fiber(f).unwrap().state,
        FiberState::Running,
        "relate 应自动恢复 WAITING fiber"
    );

    // 恢复后提交删除 Effect + 关系投影 → 完成
    emit(&mut k, f, "DeleteUser", None);
    k.relate(Relation::new(agent, "user-001", "deleted", "true", now()))
        .unwrap();
    k.advance(f, FiberState::Completed).unwrap();
    assert_eq!(k.fiber(f).unwrap().state, FiberState::Completed);

    // 事实层：delete 关系存在 + 事件可审计
    assert!(k.relations().any(|r| r.predicate == "deleted"));
    assert!(k.events().iter().any(|e| e.ty == "fiber.resumed"));
    assert!(k.events().iter().any(|e| e.ty == "effect.committed"));
}

// ── TC-K2：P1 补偿链 — A✓B✓C✗ → 逆序补偿 C⁻¹B⁻¹A⁻¹ → Compensated ─────────
#[tokio::test]
async fn tc_k2_compensation_chain_reverse_order() {
    let recorder = Arc::new(RecordingCompensator::new());
    let mut k = Kernel::new().with_compensator(recorder.clone());
    let world = k.create_context(None, Scope::World, "world").unwrap();
    let f = running(&mut k, world, "MultiEffect");

    emit(&mut k, f, "A", Some("UndoA"));
    emit(&mut k, f, "B", Some("UndoB"));
    emit(&mut k, f, "C", Some("UndoC"));

    // C 失败 → Fiber FAILED → 触发补偿
    k.advance(f, FiberState::Failed).unwrap();
    CompensationEngine::new(&mut k).compensate(f).unwrap();

    // 逆序：UndoC → UndoB → UndoA
    let calls: Vec<String> = recorder
        .calls
        .lock()
        .unwrap()
        .iter()
        .map(|(a, _)| a.clone())
        .collect();
    assert_eq!(calls, vec!["UndoC", "UndoB", "UndoA"], "补偿必须逆序");
    assert_eq!(k.fiber(f).unwrap().state, FiberState::Compensated);
    // Effect 状态全部标记补偿
    assert!(
        k.committed_effects(f)
            .iter()
            .all(|e| e.status == openaip_os::kernel::effect::EffectState::Compensated)
    );
}

// ── TC-K3：补偿失败 → Escalated + 事实 Relation（升级人工，可审计） ───────
#[tokio::test]
async fn tc_k3_compensation_failure_escalates() {
    let recorder = Arc::new(RecordingCompensator::new().with_failure("UndoB"));
    let mut k = Kernel::new().with_compensator(recorder.clone());
    let world = k.create_context(None, Scope::World, "world").unwrap();
    let f = running(&mut k, world, "FailChain");

    emit(&mut k, f, "A", Some("UndoA"));
    emit(&mut k, f, "B", Some("UndoB"));
    k.advance(f, FiberState::Failed).unwrap();

    let err = CompensationEngine::new(&mut k).compensate(f).unwrap_err();
    assert!(
        err.to_string().contains("升级人工"),
        "补偿失败应升级人工: {err}"
    );
    assert_eq!(k.fiber(f).unwrap().state, FiberState::Escalated);
    // 事实：Effect B --compensation_failed--> true
    assert!(k.relations().any(|r| r.predicate == "compensation_failed"));
    assert!(
        k.events()
            .iter()
            .any(|e| e.ty == "effect.compensation_failed")
    );
}

// ── TC-K4：补偿触发前置 — 非 Failed 状态拒绝补偿 ──────────────────────────
#[tokio::test]
async fn tc_k4_compensation_requires_failed() {
    let mut k = Kernel::new();
    let world = k.create_context(None, Scope::World, "world").unwrap();
    let f = running(&mut k, world, "T");
    emit(&mut k, f, "A", Some("UndoA"));
    // RUNNING 状态不能补偿
    assert!(matches!(
        CompensationEngine::new(&mut k).compensate(f),
        Err(KernelError::InvalidTransition(_, _))
    ));
}

// ── TC-K5：Context 销毁终止 WAITING fiber（P2：不补偿） ───────────────────
#[tokio::test]
async fn tc_k5_dispose_terminates_waiting_without_compensation() {
    let mut k = Kernel::new();
    let world = k.create_context(None, Scope::World, "world").unwrap();
    let f = running(&mut k, world, "WaitingTask");
    emit(&mut k, f, "CreateOrder", Some("CancelOrder"));
    k.wait_on(
        f,
        WaitPattern {
            source: None,
            predicate: "Approval".into(),
            target: None,
        },
    )
    .unwrap();
    k.advance(f, FiberState::Waiting).unwrap();

    k.dispose_context(world).unwrap();
    assert_eq!(k.context(world).unwrap().state, ContextState::Disposed);
    assert_eq!(k.fiber(f).unwrap().state, FiberState::Completed);
    // 不隐式补偿：Effect 保持 Committed
    assert_eq!(
        k.committed_effects(f)[0].status,
        openaip_os::kernel::effect::EffectState::Committed
    );
}

// ── TC-K6：持久化往返 — save → restore → 状态与 id 计数一致 ───────────────
#[tokio::test]
async fn tc_k6_persistence_roundtrip() {
    use openaip_os::kernel::persistence::Persistence;

    let path = std::env::temp_dir().join(format!("kernel_v2_{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);

    {
        let mut k = Kernel::new().with_compensator(Arc::new(RecordingCompensator::new()));
        let world = k.create_context(None, Scope::World, "world").unwrap();
        let f = running(&mut k, world, "P");
        emit(&mut k, f, "A", Some("UndoA"));
        k.relate(Relation::new(world, "x", "owns", "y", now()))
            .unwrap();

        let p = Persistence::open(path.to_str().unwrap()).await.unwrap();
        p.save(&k).await.unwrap();
    }

    // 恢复
    let p = Persistence::open(path.to_str().unwrap()).await.unwrap();
    let k2 = p.restore().await.unwrap();
    assert_eq!(k2.all_contexts().count(), 1);
    assert_eq!(k2.all_fibers().count(), 1);
    assert_eq!(k2.all_effects().count(), 1);
    assert_eq!(k2.all_relations().count(), 1);
    assert_eq!(k2.events().len(), 2, "effect.committed + relation.created");

    // id 计数恢复：新 context 不冲突
    let mut k3 = k2;
    let c2 = k3.create_context(None, Scope::Tenant, "t2").unwrap();
    let f2 = running(&mut k3, c2, "T2");
    emit(&mut k3, f2, "B", None);
    assert_eq!(k3.all_contexts().count(), 2);
    assert_eq!(k3.all_effects().count(), 2);

    let _ = std::fs::remove_file(&path);
}

// ── TC-K7：WaitPattern 通配匹配（source/target 通配，predicate 精确） ─────
#[tokio::test]
async fn tc_k7_wait_pattern_wildcard() {
    let mut k = Kernel::new();
    let world = k.create_context(None, Scope::World, "world").unwrap();
    let f1 = running(&mut k, world, "W1");
    let f2 = running(&mut k, world, "W2");

    // f1 等任意 Approval；f2 等特定 target 的 Approval
    k.wait_on(
        f1,
        WaitPattern {
            source: None,
            predicate: "Approval".into(),
            target: None,
        },
    )
    .unwrap();
    k.wait_on(
        f2,
        WaitPattern {
            source: None,
            predicate: "Approval".into(),
            target: Some("boss".into()),
        },
    )
    .unwrap();
    k.advance(f1, FiberState::Waiting).unwrap();
    k.advance(f2, FiberState::Waiting).unwrap();

    // 一条 Approval（target=admin）→ 只恢复 f1
    k.relate(Relation::new(world, "approval", "Approval", "admin", now()))
        .unwrap();
    assert_eq!(k.fiber(f1).unwrap().state, FiberState::Running);
    assert_eq!(
        k.fiber(f2).unwrap().state,
        FiberState::Waiting,
        "target 不匹配不得恢复"
    );

    // target=boss 的 Approval → 恢复 f2
    k.relate(Relation::new(world, "approval", "Approval", "boss", now()))
        .unwrap();
    assert_eq!(k.fiber(f2).unwrap().state, FiberState::Running);
}
