//! OAB V1.0 契约测试：Agent Communication Fabric（四 ABI）。
//!
//! 契约：docs/03_架构设计/agent-fabric-v1.md
//! - tc_f1 AgentMessage 规范化（统一消息语义，非 DSH/Codex 直传）
//! - tc_f2 Router：Intent → Capability → Policy → Agent Selection（动态选择）
//! - tc_f3 Session 生命周期（participants / messages / state）
//! - tc_f4 事件完整性（通信黑匣子：全量入 Event Log，run_id=None）
//! - tc_f5 闭环 demo：Router → Adapter → result → Event → Projection → World State

use openaip_os::compose::task::WorldEventSink;
use openaip_os::fabric::adapter::{AgentAdapter, AgentCapability};
use openaip_os::fabric::router::{FabricRouter, RouteVerdict};
use openaip_os::fabric::session::{SessionManager, SessionState};
use openaip_os::fabric::{AgentMessage, Bridge, FabricError, MessageKind};
use openaip_os::kernel::{Kernel, Scope};
use openaip_os::projection::Projection;
use openaip_os::trace::TraceEvent;
use serde_json::Value;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct VecSink(Mutex<Vec<TraceEvent>>);

impl WorldEventSink for VecSink {
    fn append(&self, ev: &TraceEvent) -> Result<(), String> {
        self.0.lock().unwrap().push(ev.clone());
        Ok(())
    }
}

/// Fake Codex 适配器：能力市场声明 coding.modify；受理后返回固定完成结果。
struct FakeCodex;

impl AgentAdapter for FakeCodex {
    fn id(&self) -> &str {
        "codex"
    }
    fn connect(&self) -> Result<(), FabricError> {
        Ok(())
    }
    fn discover(&self) -> Vec<AgentCapability> {
        vec![AgentCapability {
            name: "coding.modify".into(),
            agent_id: "codex".into(),
            quality: 0.95,
            risk: 2,
        }]
    }
    fn send(&self, msg: &AgentMessage) -> Result<AgentMessage, FabricError> {
        assert_eq!(
            msg.kind,
            MessageKind::TaskRequest,
            "Adapter 只受理 task.request"
        );
        Ok(AgentMessage {
            id: format!("m_{}", msg.id),
            session_id: msg.session_id.clone(),
            sender: "codex".into(),
            receiver: msg.sender.clone(),
            kind: MessageKind::TaskResult,
            intent: msg.intent.clone(),
            payload: serde_json::json!({
                "status": "completed",
                "files_changed": 4,
                "tests": "passed",
                "object": "order-001",
            }),
            context: Value::Null,
            capability: msg.capability.clone(),
            correlation_id: msg.correlation_id.clone(),
            causation_id: Some(msg.id.clone()),
            timestamp: msg.timestamp + 1,
        })
    }
    fn cancel(&self, _correlation_id: &str) -> Result<(), FabricError> {
        Ok(())
    }
}

fn msg(kind: MessageKind, session_id: &str, sender: &str, receiver: &str) -> AgentMessage {
    AgentMessage {
        id: format!("m_{}_{}", sender, kind.as_str()),
        session_id: session_id.into(),
        sender: sender.into(),
        receiver: receiver.into(),
        kind,
        intent: Some("modify_code".into()),
        payload: Value::Null,
        context: Value::Null,
        capability: Some("coding.modify".into()),
        correlation_id: Some("c_1".into()),
        causation_id: None,
        timestamp: 1,
    }
}

// ── tc_f1：AgentMessage 规范化 ────────────────────────────────────────
#[test]
fn tc_f1_agent_message_normalized_shape() {
    // 语义即契约：kind 序列化 / correlation / session 必填语义
    let m = msg(MessageKind::TaskRequest, "A001", "dsh", "codex");
    assert_eq!(m.kind.as_str(), "task.request");
    assert_eq!(MessageKind::TaskAccepted.as_str(), "task.accepted");
    assert_eq!(MessageKind::TaskProgress.as_str(), "task.progress");
    assert_eq!(MessageKind::TaskResult.as_str(), "task.result");
    assert_eq!(m.correlation_id.as_deref(), Some("c_1"));
    assert_eq!(m.session_id, "A001");
    // 往返序列化（wire 格式稳定：消息是通信层的唯一真相）
    let wire = serde_json::to_value(&m).unwrap();
    let back: AgentMessage = serde_json::from_value(wire).unwrap();
    assert_eq!(back.sender, "dsh");
    assert_eq!(back.receiver, "codex");
    assert_eq!(back.capability.as_deref(), Some("coding.modify"));
}

// ── tc_f2：Router — Intent → Capability → Policy → Agent Selection ─────
#[test]
fn tc_f2_router_intent_to_capability_to_policy_to_selection() {
    let mut r = FabricRouter::new();
    r.register_intent("modify_code", "coding.modify");
    r.register_intent("run_tests", "code.test");
    r.register_intent("run_shell", "shell.execute");
    r.register_agent("coding.modify", "codex", 1);
    r.register_agent("coding.modify", "claude", 2);
    // code.test 不注册提供方：验证意图已知但无提供方 → 拒绝（fail-closed）
    r.mark_high_risk("shell.execute");

    // 意图 → 能力 → 策略(allow) → 选择（priority 序：codex）
    let plan = r.route("modify_code").unwrap();
    assert_eq!(plan.capability, "coding.modify");
    assert_eq!(plan.agent_id, "codex");
    assert!(matches!(plan.verdict, RouteVerdict::Allow));

    // 动态选择：codex 下线 → claude 顶上（Codex 是 Capability Provider，不是特殊公民）
    r.unregister_agent("coding.modify", "codex");
    let plan = r.route("modify_code").unwrap();
    assert_eq!(plan.agent_id, "claude");

    // HIGH risk → Policy 在 Router 前面：require_approval
    r.register_agent("shell.execute", "codex", 1);
    let plan = r.route("run_shell").unwrap();
    assert_eq!(plan.capability, "shell.execute");
    assert!(matches!(plan.verdict, RouteVerdict::RequiresApproval));

    // 未知意图 → 拒绝（fail-closed）
    assert!(r.route("fly_to_mars").is_err());
    // 意图已知但无提供方 → 拒绝
    assert!(r.route("run_tests").is_err(), "无 code.test 提供方");
}

// ── tc_f3：Session 生命周期 ────────────────────────────────────────────
#[test]
fn tc_f3_session_lifecycle() {
    let mut sm = SessionManager::new();
    let s = sm.create(vec!["dsh".into(), "codex".into()]);
    let sid = s.id.clone();
    assert_eq!(s.state, SessionState::Active);
    assert_eq!(s.participants, vec!["dsh", "codex"]);

    // 消息累积：request → accepted → progress → result 全序
    {
        let session = sm.get_mut(&sid).unwrap();
        session
            .append(msg(MessageKind::TaskRequest, &sid, "dsh", "codex"))
            .unwrap();
        session
            .append(msg(MessageKind::TaskAccepted, &sid, "codex", "dsh"))
            .unwrap();
        session
            .append(msg(MessageKind::TaskProgress, &sid, "codex", "dsh"))
            .unwrap();
        session
            .append(msg(MessageKind::TaskResult, &sid, "codex", "dsh"))
            .unwrap();
        assert_eq!(session.messages().len(), 4);
        let kinds: Vec<&str> = session.messages().iter().map(|m| m.kind.as_str()).collect();
        assert_eq!(
            kinds,
            vec![
                "task.request",
                "task.accepted",
                "task.progress",
                "task.result"
            ]
        );
        // pause / resume
        session.pause();
        assert_eq!(session.state, SessionState::Paused);
        session.resume();
        assert_eq!(session.state, SessionState::Active);
        session.complete();
        assert_eq!(session.state, SessionState::Completed);
        // 终态拒绝追加
        assert!(
            session
                .append(msg(MessageKind::TaskProgress, &sid, "codex", "dsh"))
                .is_err()
        );
    }

    // cancel 语义：取消会话（终态）
    let s2 = sm.create(vec!["dsh".into()]);
    let s2id = s2.id.clone();
    sm.get_mut(&s2id).unwrap().cancel();
    assert_eq!(sm.get(&s2id).unwrap().state, SessionState::Cancelled);
}

// ── tc_f4：事件完整性（通信黑匣子） ────────────────────────────────────
#[tokio::test]
async fn tc_f4_all_communication_events_land_in_event_log() {
    let sink = Arc::new(VecSink::default());
    let mut r = FabricRouter::new();
    r.register_intent("modify_code", "coding.modify");
    r.register_agent("coding.modify", "codex", 1);
    let mut b = Bridge::new(r, Some(sink.clone()));
    b.connect_agent(Arc::new(FakeCodex)).unwrap();

    let mut sm = SessionManager::new();
    let s = sm.create(vec!["dsh".into(), "codex".into()]);
    let result = b
        .submit(
            &mut sm,
            &s.id,
            "dsh",
            "modify_code",
            serde_json::json!({ "repository": "demo", "task": "修复订单计算错误" }),
        )
        .unwrap();
    assert_eq!(result.kind, MessageKind::TaskResult);
    assert_eq!(result.payload["status"], "completed");
    assert_eq!(result.payload["files_changed"], 4);
    assert!(result.correlation_id.is_some(), "correlation 必须贯通");

    // 通信黑匣子：全量事件在场（Event Log 是单一扇出源）
    let evs = sink.0.lock().unwrap();
    let types: Vec<&str> = evs.iter().map(|e| e.event_type.as_str()).collect();
    for expect in [
        "fabric.agent_connected",
        "fabric.capability_invoked",
        "fabric.task_requested",
        "fabric.message_sent",
        "fabric.task_completed",
    ] {
        assert!(types.contains(&expect), "缺事件 {expect}: {types:?}");
    }
    // 事实不依赖 Run（P5 约定：run_id=None）
    assert!(evs.iter().all(|e| e.run_id.is_none()));
    // 会话消息与事件一一对应（Session 是消息的真相源，事件是其投影）
    let session = sm.get(&s.id).unwrap();
    assert_eq!(session.messages().len(), 2, "request + result");
}

// ── tc_f5：闭环 demo — Router→Adapter→result→Event→Projection→World State ──
#[tokio::test]
async fn tc_f5_full_demo_chain_to_world_state() {
    let sink = Arc::new(VecSink::default());
    let mut r = FabricRouter::new();
    r.register_intent("modify_code", "coding.modify");
    r.register_agent("coding.modify", "codex", 1);
    let mut b = Bridge::new(r, Some(sink.clone()));
    b.connect_agent(Arc::new(FakeCodex)).unwrap();

    let mut sm = SessionManager::new();
    let s = sm.create(vec!["dsh".into(), "codex".into()]);
    let result = b
        .submit(
            &mut sm,
            &s.id,
            "dsh",
            "modify_code",
            serde_json::json!({ "object": "order-001", "task": "修复金额计算" }),
        )
        .unwrap();
    assert_eq!(result.payload["status"], "completed");

    // Event Log → Projection（fabric 领域规则：task_completed → (refs[0], fabric.status, to)）
    let events = sink.0.lock().unwrap().clone();
    let mut k = Kernel::new();
    let ctx = k.create_context(None, Scope::World, "world").unwrap();
    let mut p = Projection::new(ctx).with_domain_rule("fabric.task_completed", "fabric.status");
    p.project_events(&mut k, &events).unwrap();

    // World State：order-001 的任务状态完成（Phase 5 单向流原样接续）
    assert!(
        k.relations().any(|r| r.source == "order-001"
            && r.predicate == "fabric.status"
            && r.target == "completed"),
        "World State 必须反映通信层事实"
    );
}
