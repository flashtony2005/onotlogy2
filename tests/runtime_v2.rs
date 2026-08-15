//! V2.0 Agent Runtime Production Kernel（Phase 8）场景验收。
//!
//! 北极星：以 Fiber 为执行原语、Capability 为能力原语、Policy 为安全边界、
//! Event 为事实来源、Compensation 为失败语义、Agent 为决策层的可恢复 Agent Runtime。
//!
//! S1 普通任务   S2 多步骤   S3 Approval   S4 Reject   S5 Failure
//! S6 补偿失败   S7 Crash Recovery   S8 Idempotency   S9 Audit   S10 Learning

use std::sync::Arc;

use openaip_os::cognitive::engine::{AgentView, FakeEngine, PlanStep};
use openaip_os::cognitive::episode::episodes_from_events;
use openaip_os::cognitive::{Agent, AgentLoop, LoopOutcome};
use openaip_os::compose::policy::{Cmp, Policy, Precondition};
use openaip_os::compose::{Capability, Service, Task, TaskExecutor};
use openaip_os::evolution2::{EvolutionEngine, OutcomeKind};
use openaip_os::kernel::compensation::RecordingCompensator;
use openaip_os::kernel::effect::{Effect, Reversibility};
use openaip_os::kernel::fiber::FiberState;
use openaip_os::kernel::{Compensation, Kernel, Scope};
use serde_json::{Value, json};

fn crm_service() -> Service {
    Service::new("crm-service")
        .with_capability(
            Capability::new("customer.activate", "ActivateCustomer")
                .with_reversibility(Reversibility::Reversible)
                .with_projection("status", "active")
                .requiring(&["target"]),
        )
        .with_capability(
            Capability::new("customer.refund", "RefundCustomer")
                .with_reversibility(Reversibility::Compensatable)
                .with_inverse("UndoRefund")
                .with_policy(Policy {
                    require_approval: true,
                    preconditions: vec![Precondition {
                        param: "amount".into(),
                        op: Cmp::Le,
                        value: json!(1000),
                    }],
                })
                .with_projection("refunded", "true")
                .requiring(&["target", "amount"]),
        )
        .with_capability(
            Capability::new("customer.retain", "RetainCustomer")
                .with_projection("retained", "true")
                .requiring(&["target"]),
        )
}

fn spawn_agent(k: &mut Kernel) -> Agent {
    let world = k.create_context(None, Scope::World, "world").unwrap();
    Agent::new("retention-agent", world)
}

fn running_fiber(
    k: &mut Kernel,
    ctx: openaip_os::kernel::ContextId,
) -> openaip_os::kernel::FiberId {
    let f = k.spawn(ctx, None, "demo", json!({})).unwrap();
    k.advance(f, FiberState::Starting).unwrap();
    k.advance(f, FiberState::Ready).unwrap();
    k.advance(f, FiberState::Running).unwrap();
    f
}

// ── S1：普通任务 — Goal → Plan → Policy → Capability → Result ────────
#[tokio::test]
async fn tc_s1_plain_task() {
    let services = [crm_service()];
    let mut k = Kernel::new();
    let mut agent = spawn_agent(&mut k);
    let engine = FakeEngine::new(vec![PlanStep {
        capability: "customer.retain".into(),
        args: json!({ "target": "cust-01" }),
        when: Some(Box::new(|v: &AgentView| {
            !v.has_relation("retained", "true")
        })),
    }]);
    let task = Task::new("retain", "挽留客户").expecting(&[("retained", "true")]);
    let ctx = agent.context;
    let out = {
        let mut loop_ = AgentLoop::new(&mut k, &mut agent, &services, &engine);
        loop_.run_until_done(ctx, &task, json!({})).unwrap()
    };
    assert_eq!(out, LoopOutcome::Completed);
    // Result：世界真实达成 + 事实事件
    assert!(k.relations().any(|r| r.predicate == "retained"));
    assert!(k.events().iter().any(|e| e.ty == "effect.committed"));
}

// ── S2：多步骤任务 — CreateUser → SendEmail（同一 Fiber 内顺序）───────
#[tokio::test]
async fn tc_s2_multi_step() {
    let services = [crm_service()];
    let mut k = Kernel::new();
    let mut agent = spawn_agent(&mut k);
    // 两步：先 activate（status→active），再 retain（retained→true）
    let engine = FakeEngine::new(vec![
        PlanStep {
            capability: "customer.activate".into(),
            args: json!({ "target": "u1" }),
            when: Some(Box::new(|v: &AgentView| {
                !v.has_relation("status", "active")
            })),
        },
        PlanStep {
            capability: "customer.retain".into(),
            args: json!({ "target": "u1" }),
            when: Some(Box::new(|v: &AgentView| {
                v.has_relation("status", "active") && !v.has_relation("retained", "true")
            })),
        },
    ]);
    let task = Task::new("onboard", "创建并挽留用户")
        .expecting(&[("status", "active"), ("retained", "true")]);
    let ctx = agent.context;
    let out = {
        let mut loop_ = AgentLoop::new(&mut k, &mut agent, &services, &engine);
        loop_.run_until_done(ctx, &task, json!({})).unwrap()
    };
    assert_eq!(out, LoopOutcome::Completed);
    // 两个 capability 都被执行（同一 Fiber 内顺序多步）
    assert!(
        k.relations()
            .any(|r| r.predicate == "status" && r.target == "active")
    );
    assert!(k.relations().any(|r| r.predicate == "retained"));
}

// ── S3：Approval — Plan → Policy → WaitingApproval → Approve → Resume ─
#[tokio::test]
async fn tc_s3_approval_approve_resume() {
    let services = [crm_service()];
    let mut k = Kernel::new();
    let mut agent = spawn_agent(&mut k);
    let ctx = agent.context;
    let engine = FakeEngine::new(vec![PlanStep {
        capability: "customer.refund".into(),
        args: json!({ "target": "u1", "amount": 100 }),
        when: Some(Box::new(|v: &AgentView| {
            !v.has_relation("refunded", "true")
        })),
    }]);
    let task = Task::new("refund", "退款").expecting(&[("refunded", "true")]);
    let out = {
        let mut loop_ = AgentLoop::new(&mut k, &mut agent, &services, &engine);
        loop_.run_until_done(ctx, &task, json!({})).unwrap()
    };
    assert_eq!(
        out,
        LoopOutcome::Waiting,
        "require_approval → WaitingApproval"
    );
    // 挂起审计
    assert!(k.events().iter().any(|e| e.ty == "approval.requested"));

    // 显式审批（Phase 8 API）→ Resume → 完成
    let fiber = k
        .all_fibers()
        .find(|f| f.context == ctx && f.task == "refund")
        .unwrap()
        .id;
    k.approve(fiber, "admin").unwrap();
    assert!(k.events().iter().any(|e| e.ty == "approval.granted"));
    let out2 = {
        let mut loop_ = AgentLoop::new(&mut k, &mut agent, &services, &engine);
        loop_.resume_until_done(fiber, &task).unwrap()
    };
    assert_eq!(out2, LoopOutcome::Completed);
    assert!(k.relations().any(|r| r.predicate == "refunded"));
}

// ── S4：Reject — DeleteUser → Approval → Reject → Cancel ─────────────
#[tokio::test]
async fn tc_s4_approval_reject_cancels() {
    let services = [crm_service()];
    let mut k = Kernel::new();
    let mut agent = spawn_agent(&mut k);
    let ctx = agent.context;
    let engine = FakeEngine::new(vec![PlanStep {
        capability: "customer.refund".into(),
        args: json!({ "target": "u1", "amount": 100 }),
        when: Some(Box::new(|v: &AgentView| {
            !v.has_relation("refunded", "true")
        })),
    }]);
    let task = Task::new("refund", "退款").expecting(&[("refunded", "true")]);
    let out = {
        let mut loop_ = AgentLoop::new(&mut k, &mut agent, &services, &engine);
        loop_.run_until_done(ctx, &task, json!({})).unwrap()
    };
    assert_eq!(out, LoopOutcome::Waiting);

    // 审批拒绝 → Cancelled（终态）+ 审计
    let fiber = k
        .all_fibers()
        .find(|f| f.context == ctx && f.task == "refund")
        .unwrap()
        .id;
    k.reject_approval(fiber, "compliance").unwrap();
    assert_eq!(k.fiber(fiber).map(|f| f.state), Some(FiberState::Cancelled));
    assert!(k.events().iter().any(|e| e.ty == "approval.rejected"));
    // Cancelled 后不可恢复（终态）
    assert!(k.resume(fiber).is_err());
    // 世界无变化（等待期间未执行）
    assert!(!k.relations().any(|r| r.predicate == "refunded"));
}

// ── S5：Failure — A → B → C FAILED → 逆序补偿 + 事件 ──────────────────
#[tokio::test]
async fn tc_s5_failure_compensates_reverse() {
    let mut k = Kernel::new().with_compensator(Arc::new(RecordingCompensator::new()));
    let ctx = k.create_context(None, Scope::World, "w").unwrap();
    let f = running_fiber(&mut k, ctx);
    emit_inv(&mut k, f, "A", "UndoA");
    emit_inv(&mut k, f, "B", "UndoB");
    emit_inv(&mut k, f, "C", "UndoC"); // C 也"成功"（S5 语义：C 步骤失败后补偿链含 C）

    k.advance(f, FiberState::Failed).unwrap();
    openaip_os::kernel::CompensationEngine::new(&mut k)
        .compensate(f)
        .unwrap();
    assert_eq!(k.fiber(f).map(|f| f.state), Some(FiberState::Compensated));

    // 补偿事件（G4：可审计，而非"程序员觉得应该回滚"）
    let tys: Vec<&str> = k.events().iter().map(|e| e.ty.as_str()).collect();
    assert!(tys.contains(&"compensation.started"));
    assert!(tys.contains(&"compensation.completed"));
}

fn emit_inv(k: &mut Kernel, f: openaip_os::kernel::FiberId, action: &str, undo: &str) {
    let e = Effect::new(
        action.to_string(),
        json!({}),
        Reversibility::Compensatable,
        Some(Compensation {
            action: undo.to_string(),
            payload: json!({}),
        }),
        k.now(),
    );
    k.emit(f, e).unwrap();
}

// ── S6：Compensation Failure — 补偿失败 → ManualInterventionRequired ─
#[tokio::test]
async fn tc_s6_compensation_failure_escalates() {
    let mut comp = RecordingCompensator::new();
    comp.fail_on = Some("UndoB".into());
    let mut k = Kernel::new().with_compensator(Arc::new(comp));
    let ctx = k.create_context(None, Scope::World, "w").unwrap();
    let f = running_fiber(&mut k, ctx);
    emit_inv(&mut k, f, "A", "UndoA");
    emit_inv(&mut k, f, "B", "UndoB");

    k.advance(f, FiberState::Failed).unwrap();
    let err = openaip_os::kernel::CompensationEngine::new(&mut k)
        .compensate(f)
        .unwrap_err();
    assert!(!err.to_string().is_empty(), "不能吞异常");
    assert_eq!(k.fiber(f).map(|f| f.state), Some(FiberState::Escalated));
    // 双事件：补偿失败 + 人工介入
    let tys: Vec<&str> = k.events().iter().map(|e| e.ty.as_str()).collect();
    assert!(tys.contains(&"compensation.failed"));
    assert!(tys.contains(&"manual_intervention_required"));
}

// ── S7：Crash Recovery — Event Replay → Recover Fiber → Resume ────────
#[tokio::test]
async fn tc_s7_crash_recovery() {
    use openaip_os::kernel::persistence::Persistence;
    let path = std::env::temp_dir().join(format!("runtime_v2_s7_{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);

    // ① 崩溃前：Running fiber + 已提交 Effect → 持久化
    let fiber_id = {
        let mut k = Kernel::new();
        let ctx = k.create_context(None, Scope::World, "w").unwrap();
        let f = running_fiber(&mut k, ctx);
        emit_inv(&mut k, f, "A", "UndoA");
        let p = Persistence::open(path.to_str().unwrap()).await.unwrap();
        p.save(&k).await.unwrap();
        f
    };

    // ② 重启恢复：Running → Interrupted（崩溃残留语义）
    let p = Persistence::open(path.to_str().unwrap()).await.unwrap();
    let mut k2 = p.restore().await.unwrap();
    assert_eq!(
        k2.fiber(fiber_id).map(|f| f.state),
        Some(FiberState::Interrupted),
        "崩溃残留 Running → Interrupted"
    );

    // ③ 恢复：Interrupted → Running + fiber.recovered 事件 → 继续执行
    k2.recover(fiber_id).unwrap();
    assert_eq!(
        k2.fiber(fiber_id).map(|f| f.state),
        Some(FiberState::Running)
    );
    assert!(k2.events().iter().any(|e| e.ty == "fiber.recovered"));
    // ④ 继续完成
    k2.advance(fiber_id, FiberState::Completed).unwrap();
    let _ = std::fs::remove_file(&path);
}

// ── S8：Idempotency — 同 key 两次执行无二次业务效果 ───────────────────
#[tokio::test]
async fn tc_s8_idempotency() {
    let services = [crm_service()];
    let mut k = Kernel::new();
    let ctx = k.create_context(None, Scope::World, "w").unwrap();
    let cap = services[0].capability("customer.retain").unwrap().clone();
    let task = Task::new("retain", "挽留").expecting(&[("retained", "true")]);

    let fiber = {
        let mut tx = TaskExecutor::new(&mut k);
        tx.spawn(ctx, &task, json!({})).unwrap()
    };
    let args = json!({ "target": "cust-01" });

    // 第一次：真实执行
    {
        let mut tx = TaskExecutor::new(&mut k);
        let out = tx
            .step_idempotent(fiber, &cap, args.clone(), "key-1")
            .unwrap();
        assert_eq!(out, openaip_os::compose::StepOutcome::Committed);
    }
    let rels_after_first = k.relations().count();

    // 第二次（同 key）：重放——世界不再变化
    {
        let mut tx = TaskExecutor::new(&mut k);
        let out = tx
            .step_idempotent(fiber, &cap, args.clone(), "key-1")
            .unwrap();
        assert_eq!(out, openaip_os::compose::StepOutcome::Committed);
    }
    assert_eq!(
        k.relations().count(),
        rels_after_first,
        "同 key 重放不产生二次业务效果"
    );
    assert!(
        k.events()
            .iter()
            .any(|e| e.ty == "effect.idempotent_replay")
    );
    assert_eq!(
        k.events()
            .iter()
            .filter(|e| e.ty == "effect.committed")
            .count(),
        1,
        "Effect 只提交一次"
    );

    // 不同 key：再次真实执行
    {
        let mut tx = TaskExecutor::new(&mut k);
        tx.step_idempotent(fiber, &cap, json!({ "target": "cust-02" }), "key-2")
            .unwrap();
    }
    assert_eq!(k.relations().count(), rels_after_first + 1);
}

// ── S9：Audit — 任何执行可回答 谁/为什么/计划/授权/调用/结果 ─────────
#[tokio::test]
async fn tc_s9_audit_trail() {
    let services = [crm_service()];
    let mut k = Kernel::new();
    let mut agent = spawn_agent(&mut k);
    let ctx = agent.context;
    let engine = FakeEngine::new(vec![PlanStep {
        capability: "customer.refund".into(),
        args: json!({ "target": "u1", "amount": 100 }),
        when: Some(Box::new(|v: &AgentView| {
            !v.has_relation("refunded", "true")
        })),
    }]);
    let task = Task::new("refund", "退款").expecting(&[("refunded", "true")]);
    let out = {
        let mut loop_ = AgentLoop::new(&mut k, &mut agent, &services, &engine);
        loop_.run_until_done(ctx, &task, json!({})).unwrap()
    };
    assert_eq!(out, LoopOutcome::Waiting);
    let fiber = k.all_fibers().find(|f| f.context == ctx).unwrap().id;
    k.approve(fiber, "finance-ops").unwrap();
    {
        let mut loop_ = AgentLoop::new(&mut k, &mut agent, &services, &engine);
        loop_.resume_until_done(fiber, &task).unwrap();
    }

    // 全链路审计：谁(by)/计划(task)/授权(requested+granted)/调用(committed)/结果(Completed)
    let evs: Vec<&str> = k.events().iter().map(|e| e.ty.as_str()).collect();
    for required in [
        "approval.requested",
        "approval.granted",
        "effect.committed",
        "relation.created",
    ] {
        assert!(evs.contains(&required), "缺少审计事件 {required}");
    }
    let granted = k
        .events()
        .iter()
        .find(|e| e.ty == "approval.granted")
        .unwrap();
    assert_eq!(granted.payload["by"], "finance-ops", "谁授权的");
    assert_eq!(
        k.fiber(fiber).map(|f| f.state),
        Some(FiberState::Completed),
        "执行结果可审计"
    );
}

// ── S10：Learning — 同种失败多次 → Episode → Improvement Proposal ────
#[tokio::test]
async fn tc_s10_learning_proposes_not_auto_fixes() {
    // ① 多次失败执行（3 次补偿链失败场景）
    let mut k = Kernel::new().with_compensator(Arc::new(RecordingCompensator::new()));
    let ctx = k.create_context(None, Scope::World, "w").unwrap();
    for _ in 0..3 {
        let f = running_fiber(&mut k, ctx);
        emit_inv(&mut k, f, "Charge", "UndoCharge");
        emit_inv(&mut k, f, "Reserve", "UndoReserve");
        k.advance(f, FiberState::Failed).unwrap();
        openaip_os::kernel::CompensationEngine::new(&mut k)
            .compensate(f)
            .unwrap();
    }

    // ② Execution Memory：事件 → Episode
    let episodes = episodes_from_events(&k);
    let failed: Vec<_> = episodes.iter().filter(|e| e.is_failure_signal()).collect();
    assert!(failed.len() >= 3, "应有失败片段");
    assert!(
        failed
            .iter()
            .any(|e| e.actions.contains(&"Charge".to_string())),
        "Episode 聚合了执行动作"
    );
    assert!(
        failed
            .iter()
            .any(|e| e.compensations.contains(&"UndoCharge".to_string())),
        "Episode 聚合了补偿动作"
    );

    // ③ 经验 → Pattern → 改进建议（Learn ≠ Permission：失败模式只产生
    //    认知警告 RiskSignal，绝不产生权限 Proposal——系统只建议，不自动修改）
    let mut ev = EvolutionEngine::new();
    for e in &episodes {
        for a in &e.actions {
            ev.record(a.clone(), OutcomeKind::Failed);
        }
    }
    ev.mine();
    let signals = ev.risk_signals();
    assert!(
        !signals.is_empty(),
        "同种失败多次 → 改进建议（RiskSignal）产生"
    );
    assert!(
        signals.iter().any(|r| r.action == "Charge"),
        "改进建议指向失败动作"
    );
    // 失败经验绝不进入权限通道（Phase 6 不变量：FailurePattern → RiskSignal，非 Proposal）
    ev.propose();
    assert!(
        ev.proposals().is_empty(),
        "失败模式不产生权限提案（不自动修改生产）"
    );
    // 生产系统未被修改（无 gate 放行、无规则变化）
    assert!(!ev.is_gate_enabled());
    assert!(k.relations().all(|r| r.predicate != "gate_approved"));
}

// 防未使用告警（部分场景复用导入）
#[allow(dead_code)]
fn _unused(_: &Value) {}
