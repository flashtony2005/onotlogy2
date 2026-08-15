//! V2.0 Session Event Log（Phase 10 · 第五核心）契约测试。
//!
//! S1 SessionEvent append-only：不可 update/delete；修正 = Correction Event
//! S2 Replay：事件序列重建时间线（User → Skill → Capability → Plugin → Result → Error → Retry）
//! S3 Model-visible ⇒ Logged：context.injected 必有来源事件
//! S4 Fork：Session A 边界分叉 A1/A2
//! S5 端到端：User Goal → Skill → Capability → Plugin → AI/Tool → Result → Session Log → Replay

use openaip_os::session::event::{EventType, SessionEvent, SessionId};
use openaip_os::session::log::SessionLog;
use serde_json::json;

#[tokio::test]
async fn tc_s1_append_only_and_correction() {
    let mut log = SessionLog::new(SessionId::new("s1"));
    let seq1 = log.append(EventType::SessionCreated, json!({}));
    let seq2 = log.append(EventType::UserMessage, json!({ "text": "你好" }));
    assert_eq!(seq1, 0, "seq 从 0 单调递增");
    assert_eq!(seq2, 1);
    assert_eq!(log.len(), 2);

    // append-only：无 update/delete API（编译期保证）——事件不可变
    let events = log.events();
    assert_eq!(events[0].event_type, EventType::SessionCreated);
    assert_eq!(events[0].seq, 0);
    assert_eq!(events[0].session_id.0, "s1");

    // 修正 = 追加 Correction Event（不改旧事件）
    let seq3 = log.append(
        EventType::UserMessage,
        json!({ "text": "你好（更正：你好世界）", "correction_of": seq2 }),
    );
    assert_eq!(seq3, 2, "修正事件追加而非修改");
    assert_eq!(log.events()[1].data["text"], "你好", "旧事件不变");
}

#[tokio::test]
async fn tc_s2_replay_rebuilds_timeline() {
    let mut log = SessionLog::new(SessionId::new("s1"));
    log.append(EventType::SessionCreated, json!({}));
    log.append(EventType::UserMessage, json!({ "text": "分析项目" }));
    log.append(EventType::SkillSelected, json!({ "skill": "CodeAnalysis" }));
    log.append(
        EventType::CapabilitySelected,
        json!({ "capability": "repository.read" }),
    );
    log.append(EventType::PluginCall, json!({ "plugin": "git" }));
    log.append(EventType::PluginResult, json!({ "ok": true }));
    log.append(
        EventType::ArtifactCreated,
        json!({ "artifact": "ProjectOntology" }),
    );
    log.append(EventType::TestStarted, json!({}));
    log.append(EventType::TestResult, json!({ "passed": false }));
    log.append(EventType::Error, json!({ "message": "test failed" }));
    log.append(EventType::TurnCompleted, json!({}));

    // Replay：完整时间线可重建
    let replay = log.replay();
    assert_eq!(replay.len(), 11);
    let tys: Vec<&str> = replay.iter().map(|e| e.event_type.as_str()).collect();
    assert_eq!(
        tys,
        vec![
            "session.created",
            "user.message",
            "skill.selected",
            "capability.selected",
            "plugin.call",
            "plugin.result",
            "artifact.created",
            "test.started",
            "test.result",
            "error",
            "turn.completed",
        ],
        "时间线顺序可重建"
    );
    // seq 连续性
    for (i, e) in replay.iter().enumerate() {
        assert_eq!(e.seq, i as u64);
    }
}

#[tokio::test]
async fn tc_s3_model_visible_logged() {
    let mut log = SessionLog::new(SessionId::new("s1"));
    // 进入模型上下文的信息必须先在日志中有来源
    log.append(
        EventType::ContextInjected,
        json!({ "artifact": "ProjectOntology" }),
    );
    log.append(EventType::UserMessage, json!({ "text": "继续" }));
    log.append(EventType::AssistantMessage, json!({ "text": "分析完成" }));

    let ctx_events: Vec<&SessionEvent> = log
        .events()
        .iter()
        .filter(|e| e.event_type == EventType::ContextInjected)
        .collect();
    assert!(
        !ctx_events.is_empty(),
        "context.injected 事件存在（Model-visible ⇒ Logged）"
    );
    assert_eq!(ctx_events[0].data["artifact"], "ProjectOntology");
}

#[tokio::test]
async fn tc_s4_fork_session() {
    let mut a = SessionLog::new(SessionId::new("A"));
    a.append(EventType::SessionCreated, json!({}));
    a.append(EventType::UserMessage, json!({ "text": "任务" }));
    a.append(EventType::SkillSelected, json!({ "skill": "CodeAnalysis" }));

    // 稳定边界 Fork → A1 / A2（实验基础：GPT vs Claude vs Qwen）
    let mut a1 = a.fork(SessionId::new("A1"));
    let mut a2 = a.fork(SessionId::new("A2"));
    a1.append(
        EventType::CapabilitySelected,
        json!({ "capability": "code.modify", "agent": "GPT" }),
    );
    a2.append(
        EventType::CapabilitySelected,
        json!({ "capability": "code.modify", "agent": "Claude" }),
    );

    // 分叉共享历史 + 独立延续
    assert_eq!(a1.events().len(), 4);
    assert_eq!(a2.events().len(), 4);
    // 历史事实不可变：边界前的事件保留原归属（Append-only 原则延伸）
    assert_eq!(
        a1.events()[0].session_id.0,
        "A",
        "历史事件归属原 Session（不可变）"
    );
    assert_eq!(
        a1.events()[2].event_type,
        EventType::SkillSelected,
        "共享历史"
    );
    // 分叉后的新事件归属新 Session
    assert_eq!(a1.events()[3].session_id.0, "A1");
    assert_eq!(a2.events()[3].session_id.0, "A2");
    assert_eq!(a.events().len(), 3, "原 Session 不变");
}

#[tokio::test]
async fn tc_s5_end_to_end_session() {
    // 完整软件工程 Session（定稿 §21 序列的浓缩版）
    let mut log = SessionLog::new(SessionId::new("swe-1"));
    log.append(EventType::SessionCreated, json!({}));
    log.append(
        EventType::UserMessage,
        json!({ "text": "实现订单退款功能并完成测试" }),
    );
    log.append(EventType::SkillSelected, json!({ "skill": "CodeAnalysis" }));
    log.append(
        EventType::CapabilitySelected,
        json!({ "capability": "repository.read" }),
    );
    log.append(EventType::PluginCall, json!({ "plugin": "GitPlugin" }));
    log.append(EventType::PluginResult, json!({ "ok": true }));
    log.append(
        EventType::ArtifactCreated,
        json!({ "artifact": "ProjectOntology" }),
    );
    log.append(
        EventType::ApprovalRequested,
        json!({ "requirement": "approval.manager" }),
    );
    log.append(
        EventType::ApprovalResolved,
        json!({ "decision": "approved" }),
    );
    log.append(
        EventType::CapabilitySelected,
        json!({ "capability": "code.modify" }),
    );
    log.append(EventType::CodeChanged, json!({ "file": "refund.rs" }));
    log.append(EventType::TestStarted, json!({}));
    log.append(EventType::PluginCall, json!({ "plugin": "MavenPlugin" }));
    log.append(EventType::TestResult, json!({ "passed": false }));
    log.append(EventType::Error, json!({ "message": "test failed" }));
    log.append(
        EventType::CapabilitySelected,
        json!({ "capability": "failure.analyze" }),
    );
    log.append(EventType::CodeChanged, json!({ "file": "refund.rs" }));
    log.append(EventType::TestResult, json!({ "passed": true }));
    log.append(
        EventType::ArtifactCreated,
        json!({ "artifact": "EngineeringReport" }),
    );
    log.append(EventType::TurnCompleted, json!({}));

    // Replay 完整闭环：User → Skill → Capability → Plugin → AI/Tool → Result → Log → Replay
    let replay = log.replay();
    assert_eq!(replay.len(), 20);
    let tys: Vec<&str> = replay.iter().map(|e| e.event_type.as_str()).collect();
    assert!(tys.contains(&"approval.requested") && tys.contains(&"approval.resolved"));
    // 失败 → 重试 → 成功可见
    let test_results: Vec<&str> = tys
        .iter()
        .filter(|t| **t == "test.result")
        .copied()
        .collect();
    assert_eq!(test_results.len(), 2, "失败 + 重试成功两条记录");
    assert_eq!(replay[13].data["passed"], false);
    assert_eq!(replay[17].data["passed"], true);
}
