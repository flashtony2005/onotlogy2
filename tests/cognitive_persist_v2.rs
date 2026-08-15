//! V2.0 认知状态持久化（Phase 10 收口）契约测试。
//!
//! CP1 Memory roundtrip
//! CP2 EvolutionEngine 快照（经验/权限/开关）roundtrip + 重建后管线可用
//! CP3 RuleStore + RuleStatsStore roundtrip
//! CP4 SkillRegistry roundtrip（含生命周期状态）
//! CP5 SessionLog roundtrip（append-only 日志可恢复）
//! CP6 PluginRegistry 状态快照 roundtrip（manifest + lifecycle）
//! CP7 缺失 key → None；删除生效

use openaip_os::cognitive::Memory;
use openaip_os::cognitive::persist::CognitiveStore;
use openaip_os::evolution2::{EvolutionEngine, OutcomeKind};
use openaip_os::plugin::manifest::{PluginManifest, RuntimeType};
use openaip_os::plugin::registry::{PluginRegistry, PluginStatus};
use openaip_os::rule::RuleStatsStore;
use openaip_os::session::event::{EventType, SessionId};
use openaip_os::session::log::SessionLog;
use openaip_os::skill::asset::{SkillAsset, SkillStatus};
use openaip_os::skill::registry::SkillRegistry;
use serde_json::{Value, json};

async fn store() -> CognitiveStore {
    CognitiveStore::open(":memory:").await.unwrap()
}

#[tokio::test]
async fn tc_cp1_memory_roundtrip() {
    let s = store().await;
    let mut m = Memory::new();
    m.record(
        "ActivateCustomer",
        json!({ "capability": "customer.activate" }),
    );
    m.record(
        "ActivateCustomer",
        json!({ "capability": "customer.activate" }),
    );

    s.save("memory", &m).await.unwrap();
    let back: Memory = s.load("memory").await.unwrap().expect("恢复");
    assert_eq!(back.hits_of("ActivateCustomer"), 2, "hits 保留");
}

#[tokio::test]
async fn tc_cp2_evolution_snapshot_roundtrip() {
    let s = store().await;
    let mut ev = EvolutionEngine::new().with_gate_enabled(true);
    ev.record("RefundCustomer", OutcomeKind::Success);
    ev.record("RefundCustomer", OutcomeKind::Success);
    ev.record("RefundCustomer", OutcomeKind::Success);
    ev.record("RefundCustomer", OutcomeKind::Success);
    ev.record("RefundCustomer", OutcomeKind::Success);
    ev.mine();
    ev.propose();
    ev.validate();
    assert!(ev.gate(
        "RefundCustomer",
        openaip_os::kernel::Reversibility::Compensatable
    ));

    let snap = ev.snapshot();
    s.save("evolution", &snap).await.unwrap();
    let back: openaip_os::evolution2::EvolutionSnapshot =
        s.load("evolution").await.unwrap().unwrap();
    let mut restored = EvolutionEngine::restore(back);
    // 经验/权限/开关保留
    assert_eq!(restored.experience("RefundCustomer").unwrap().success, 5);
    assert_eq!(
        restored.permission("RefundCustomer").map(|p| p.state),
        Some(openaip_os::evolution2::EvolutionPermissionState::Validated)
    );
    assert!(restored.is_gate_enabled());
    // 重建后管线可用（mine/propose/validate 重新生成模式）
    restored.mine();
    restored.propose();
    restored.validate();
    assert!(restored.gate(
        "RefundCustomer",
        openaip_os::kernel::Reversibility::Compensatable
    ));
}

#[tokio::test]
async fn tc_cp3_rule_store_roundtrip() {
    let s = store().await;
    let mut stats = RuleStatsStore::new();
    stats.record("r1", OutcomeKind::Success);
    stats.record("r1", OutcomeKind::Failed);
    s.save("rule_stats", &stats).await.unwrap();
    let back: RuleStatsStore = s.load("rule_stats").await.unwrap().unwrap();
    assert_eq!(back.stats_of("r1").unwrap().success, 1);
    assert_eq!(back.stats_of("r1").unwrap().failure_signal(), 1);

    // RuleStore（行为规则，已有 serde）
    let store = openaip_os::rule::RuleStore::new().with_rule(openaip_os::rule::BehaviorRule::new(
        "r1",
        "customer.activate",
        json!({}),
        openaip_os::rule::ConditionExpr::HasRelation {
            predicate: "status".into(),
            target: "active".into(),
        },
    ));
    s.save("rules", &store).await.unwrap();
    let back2: openaip_os::rule::RuleStore = s.load("rules").await.unwrap().unwrap();
    assert_eq!(back2.rule("r1").unwrap().version, 1);
}

#[tokio::test]
async fn tc_cp4_skill_registry_roundtrip() {
    let s = store().await;
    let mut registry = SkillRegistry::new();
    // 最小 SkillAsset（serde 全链路）
    let asset = SkillAsset {
        id: "demo".into(),
        name: "Demo".into(),
        version: "1.0".into(),
        skill_type: openaip_os::skill::ir::SkillType::Atomic,
        goal: "目标".into(),
        inputs: vec![],
        outputs: vec![],
        workflow: vec![],
        capabilities: vec!["web.search".into()],
        artifacts: vec![],
        preconditions: vec![],
        postconditions: vec![],
        policies: vec![],
        metrics: vec![],
        feedback_loops: vec![],
        provenance: openaip_os::skill::ir::Provenance {
            raw_skill_id: "demo".into(),
            parser: "test".into(),
        },
        status: SkillStatus::Active,
        lineage: openaip_os::skill::lineage::Lineage::new("1.0"),
    };
    registry.register(asset).unwrap();

    s.save("skills", &registry).await.unwrap();
    let back: SkillRegistry = s.load("skills").await.unwrap().unwrap();
    let a = back.get("demo").unwrap();
    assert_eq!(a.capabilities, vec!["web.search"]);
    assert_eq!(a.status, SkillStatus::Active, "生命周期状态保留");
}

#[tokio::test]
async fn tc_cp5_session_log_roundtrip() {
    let s = store().await;
    let mut log = SessionLog::new(SessionId::new("s1"));
    log.append(EventType::SessionCreated, json!({}));
    log.append(EventType::UserMessage, json!({ "text": "你好" }));
    log.append(EventType::PluginCall, json!({ "plugin": "http.echo" }));

    s.save("session.s1", &log).await.unwrap();
    let back: SessionLog = s.load("session.s1").await.unwrap().unwrap();
    assert_eq!(back.len(), 3, "append-only 日志完整恢复");
    let tys: Vec<&str> = back
        .replay()
        .iter()
        .map(|e| e.event_type.as_str())
        .collect();
    assert_eq!(tys, vec!["session.created", "user.message", "plugin.call"]);
}

#[tokio::test]
async fn tc_cp6_plugin_registry_state_roundtrip() {
    let s = store().await;
    let mut reg = PluginRegistry::new();
    // 用 LocalFunctionPlugin 构造
    let m2 = PluginManifest::new("local.file", "File", "1.0.0", RuntimeType::Local)
        .with_capability("file.read");
    reg.install(Box::new(
        openaip_os::plugin::exec::LocalFunctionPlugin::new(m2, Box::new(|_, _| Ok(json!({})))),
    ))
    .unwrap();
    reg.validate("local.file").unwrap();
    reg.register("local.file").unwrap();
    reg.activate("local.file").unwrap();

    let snap = reg.snapshot();
    s.save("plugins", &snap).await.unwrap();
    let back: openaip_os::plugin::registry::PluginStateSnapshot =
        s.load("plugins").await.unwrap().unwrap();
    let mut reg2 = PluginRegistry::new();
    reg2.restore_state(back);
    assert_eq!(reg2.status("local.file"), Some(PluginStatus::Active));
    assert_eq!(
        reg2.get("local.file")
            .unwrap()
            .manifest()
            .capabilities
            .len(),
        1
    );
}

#[tokio::test]
async fn tc_cp7_missing_and_delete() {
    let s = store().await;
    let none: Option<Value> = s.load("missing").await.unwrap();
    assert!(none.is_none(), "缺失 key → None");

    s.save("k", &json!({"a": 1})).await.unwrap();
    s.delete("k").await.unwrap();
    let gone: Option<Value> = s.load("k").await.unwrap();
    assert!(gone.is_none(), "删除生效");
}
