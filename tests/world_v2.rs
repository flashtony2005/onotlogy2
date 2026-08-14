//! V2.0 Temporal World（Phase 4）集成契约：as_of / diff / replay / event sourcing。
//!
//! W1 事件日志 append-only 累积（不因 save 重写丢失历史）
//! W2 as_of(t)：时间窗重建（expire 闭合后旧时刻仍可见）
//! W3 diff(t1,t2)：区间世界变化
//! W4 replay(fiber)：fiber 事件序列重放
//! W5 持久化往返后 as_of 正确

use openaip_os::kernel::effect::{Effect, Reversibility};
use openaip_os::kernel::fiber::FiberState;
use openaip_os::kernel::persistence::Persistence;
use openaip_os::kernel::relation::Relation;
use openaip_os::kernel::{Kernel, Scope};
use openaip_os::world::World;
use serde_json::json;

fn temp_db(name: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("{name}_{}.db", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

/// 在固定时钟下创建 ctx + fiber + 提交一个 effect。
fn seed(k: &mut Kernel, ctx: openaip_os::kernel::ContextId) -> openaip_os::kernel::FiberId {
    let f = k.spawn(ctx, None, "t", json!({})).unwrap();
    k.advance(f, FiberState::Starting).unwrap();
    k.advance(f, FiberState::Ready).unwrap();
    k.advance(f, FiberState::Running).unwrap();
    let e = Effect::new(
        "CreateOrder",
        json!({ "amount": 100 }),
        Reversibility::Compensatable,
        None,
        k.now(),
    );
    k.emit(f, e).unwrap();
    f
}

// ── W1：事件日志 append-only 累积（多次 save 不丢历史） ───────────────────
#[tokio::test]
async fn tc_w1_event_log_accumulates() {
    let path = temp_db("w1");
    let mut k = Kernel::with_clock(|| 100);
    let ctx = k.create_context(None, Scope::World, "world").unwrap();
    seed(&mut k, ctx);
    k.relate(Relation::new(ctx, "a", "owns", "b", k.now()))
        .unwrap();

    let p = Persistence::open(path.to_str().unwrap()).await.unwrap();
    p.save(&k).await.unwrap();
    let first = p.event_log().await.unwrap().len();
    assert!(
        first >= 2,
        "effect.committed + relation.created，实际 {first}"
    );

    // 第二次操作 + save：日志应累积（不重写丢失）
    let mut k2 = k;
    let ctx2 = k2.create_context(None, Scope::Tenant, "t2").unwrap();
    k2.relate(Relation::new(ctx2, "x", "serves", "y", k2.now()))
        .unwrap();
    p.save(&k2).await.unwrap();
    let second = p.event_log().await.unwrap().len();
    assert!(second > first, "日志应累积：{first} → {second}");
    let _ = std::fs::remove_file(&path);
}

// ── W2：as_of(t) 时间窗重建（expire 后旧时刻仍可见） ─────────────────────
#[tokio::test]
async fn tc_w2_as_of_time_window() {
    let path = temp_db("w2");

    // t=100：创建关系 A（valid_from=100）
    let mut k = Kernel::with_clock(|| 100);
    let ctx = k.create_context(None, Scope::World, "world").unwrap();
    let rid = k
        .relate(Relation::new(ctx, "user-001", "status", "active", k.now()))
        .unwrap();
    let p = Persistence::open(path.to_str().unwrap()).await.unwrap();
    p.save(&k).await.unwrap();

    // t=200：闭合 A 的时间窗（valid_until=200；restore 保持 id 一致）
    let k2 = p.restore().await.unwrap();
    let mut k3 = k2;
    k3.expire_relation(rid, 200).unwrap();
    p.save(&k3).await.unwrap();

    // as_of(150)：A 有效；as_of(250)：A 已过期
    let w = World::new(&p);
    let at150 = w.as_of(150).await.unwrap();
    assert!(
        at150
            .relations
            .iter()
            .any(|(s, pr, t)| s == "user-001" && pr == "status" && t == "active"),
        "t=150 应看到 A（valid 窗口内）"
    );
    let at250 = w.as_of(250).await.unwrap();
    assert!(
        !at250
            .relations
            .iter()
            .any(|(s, pr, _)| s == "user-001" && pr == "status"),
        "t=250 不应看到 A（已过期）"
    );
    let _ = std::fs::remove_file(&path);
}

// ── W3：diff(t1,t2) 区间世界变化 ──────────────────────────────────────────
#[tokio::test]
async fn tc_w3_diff_window() {
    let path = temp_db("w3");
    let mut k = Kernel::with_clock(|| 100);
    let ctx = k.create_context(None, Scope::World, "world").unwrap();
    seed(&mut k, ctx);
    k.relate(Relation::new(ctx, "a", "owns", "b", k.now()))
        .unwrap();
    let p = Persistence::open(path.to_str().unwrap()).await.unwrap();
    p.save(&k).await.unwrap();

    // t=200：新增关系 C
    let mut k2 = Kernel::with_clock(|| 200);
    let ctx2 = k2.create_context(None, Scope::World, "world2").unwrap();
    k2.relate(Relation::new(ctx2, "c", "relates", "d", k2.now()))
        .unwrap();
    p.save(&k2).await.unwrap();

    let w = World::new(&p);
    let changes = w.diff(100, 200).await.unwrap();
    // 区间内应含 relation.created（C）；不含 t=100 的旧事件
    assert!(
        changes
            .iter()
            .any(|c| c.ty == "relation.created" && c.at == 200),
        "diff 应含 t=200 的新关系事件"
    );
    assert!(
        !changes.iter().any(|c| c.at <= 100),
        "diff 不含 t=100 及之前的事件"
    );
    let _ = std::fs::remove_file(&path);
}

// ── W4：replay(fiber) 事件序列重放 ────────────────────────────────────────
#[tokio::test]
async fn tc_w4_replay_fiber() {
    let path = temp_db("w4");
    let mut k = Kernel::with_clock(|| 100);
    let ctx = k.create_context(None, Scope::World, "world").unwrap();
    let f = seed(&mut k, ctx);
    k.advance(f, FiberState::Completed).unwrap();
    let p = Persistence::open(path.to_str().unwrap()).await.unwrap();
    p.save(&k).await.unwrap();

    let w = World::new(&p);
    let seq = w.replay(&f.to_string()).await.unwrap();
    assert!(
        seq.iter().any(|c| c.ty == "effect.committed"),
        "重放应含 effect.committed"
    );
    assert!(
        seq.iter()
            .all(|c| c.fiber.as_deref() == Some(&f.to_string())),
        "重放只含该 fiber 的事件"
    );
    let _ = std::fs::remove_file(&path);
}

// ── W5：往返后 as_of 仍正确 ───────────────────────────────────────────────
#[tokio::test]
async fn tc_w5_roundtrip_as_of() {
    let path = temp_db("w5");
    let mut k = Kernel::with_clock(|| 100);
    let ctx = k.create_context(None, Scope::World, "world").unwrap();
    k.relate(Relation::new(ctx, "a", "owns", "b", k.now()))
        .unwrap();
    let p = Persistence::open(path.to_str().unwrap()).await.unwrap();
    p.save(&k).await.unwrap();

    // 恢复后再次 save（往返）
    let k2 = p.restore().await.unwrap();
    p.save(&k2).await.unwrap();

    let w = World::new(&p);
    let snap = w.as_of(100).await.unwrap();
    assert_eq!(snap.relations.len(), 1);
    assert_eq!(snap.relations[0], ("a".into(), "owns".into(), "b".into()));
    let _ = std::fs::remove_file(&path);
}
