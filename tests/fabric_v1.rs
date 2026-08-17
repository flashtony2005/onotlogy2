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
use openaip_os::facade::FabricService;
use openaip_os::kernel::{Kernel, Scope};
use openaip_os::projection::Projection;
use openaip_os::trace::TraceEvent;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
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
    fn cancel(&self, _correlation_id: &str) -> Result<bool, FabricError> {
        Ok(true) // fake：无真实子进程，总是「已处理」
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
            None,
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
            None,
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

// ── OAB v1.1：真实适配器（桥接 ProcessWorker）+ stream/cancel 真实语义 ──
// 契约 §3/§6：Adapter 管协议、Worker 是执行连接器；payload 约定 args/task；
// stream = 逐行 stdout → TaskProgress；cancel = 按 correlation_id kill。
use openaip_os::fabric::adapters::{ProcessAdapter, codex_adapter, dsh_adapter};
use openaip_os::worker::manifest::WorkerManifest;
use openaip_os::worker::process::ProcessWorker;

/// 用 /bin/sh 构造测试适配器（不依赖 codex/dsh 安装；命令可覆盖）。
fn sh_adapter(capabilities: &[&str]) -> ProcessAdapter {
    let mut m = WorkerManifest::new("sh-agent", "agent").with_capabilities(capabilities);
    m.quality = 0.9;
    m.risk = 2;
    ProcessAdapter::new("sh-agent", ProcessWorker::new(m, "/bin/sh"))
}

// ── tc_f6：真实适配器——discover 能力市场与 WorkerManifest 同源 ─────────
#[test]
fn tc_f6_real_adapters_discover_from_worker_manifest() {
    // Codex/DSH 工厂：能力市场与 codex_worker/dsh_worker 同源（不是特殊公民）
    let codex = codex_adapter(None);
    assert_eq!(codex.id(), "codex");
    let caps: Vec<String> = codex.discover().into_iter().map(|c| c.name).collect();
    for expect in ["code.read", "code.write", "code.test", "code.search"] {
        assert!(caps.iter().any(|c| c == expect), "缺 {expect}: {caps:?}");
    }
    let dsh = dsh_adapter(None);
    assert_eq!(dsh.id(), "dsh");
    let dsh_caps: Vec<String> = dsh.discover().into_iter().map(|c| c.name).collect();
    assert!(
        dsh_caps.iter().any(|c| c == "agent.task") && dsh_caps.iter().any(|c| c == "shell.execute")
    );
    // 风险/质量来自 manifest（Policy 判定依据；codex_worker 声明 quality=0.95）
    let first = codex.discover().into_iter().next().unwrap();
    assert_eq!(first.agent_id, "codex");
    assert!(
        (first.quality - 0.95).abs() < 1e-9,
        "quality 应来自 WorkerManifest"
    );
    assert!(first.risk >= 1 && first.risk <= 10);
}

// ── tc_f7：真实进程闭环（send：task.request → 进程 → task.result）────────
#[tokio::test]
async fn tc_f7_real_process_send_roundtrip_through_bridge() {
    let sink = Arc::new(VecSink::default());
    let mut r = FabricRouter::new();
    r.register_intent("run_check", "shell.execute");
    r.register_agent("shell.execute", "sh-agent", 1);
    let mut b = Bridge::new(r, Some(sink.clone()));
    // 命令不可用 → connect 失败（真实握手：PATH 探测）
    let ghost = ProcessAdapter::new(
        "ghost",
        ProcessWorker::new(
            WorkerManifest::new("ghost", "agent").with_capabilities(&["shell.execute"]),
            "no-such-cmd-xyz-123",
        ),
    );
    assert!(
        ghost.connect().is_err(),
        "命令不存在必须 connect 失败（fail-closed）"
    );
    // 真实进程：/bin/sh 可用 → connect 成功
    let sh = sh_adapter(&["shell.execute"]);
    assert!(sh.connect().is_ok());
    b.connect_agent(Arc::new(sh)).unwrap();

    let mut sm = SessionManager::new();
    let s = sm.create(vec!["dsh".into(), "sh-agent".into()]);
    let result = b
        .submit(
            &mut sm,
            &s.id,
            "dsh",
            "run_check",
            serde_json::json!({ "args": ["-c", "echo done-ok"] }),
            None,
        )
        .unwrap();
    assert_eq!(result.kind, MessageKind::TaskResult);
    assert_eq!(result.payload["status"], "completed");
    assert_eq!(result.payload["exit_code"], 0);
    assert!(
        result.payload["stdout"]
            .as_str()
            .unwrap()
            .contains("done-ok")
    );
    assert_eq!(result.sender, "sh-agent");
    assert_eq!(result.receiver, "dsh");
    // correlation 贯通（request → result）
    assert!(result.correlation_id.is_some());

    // 失败路径：非零退出 → status=failed
    let result2 = b
        .submit(
            &mut sm,
            &s.id,
            "dsh",
            "run_check",
            serde_json::json!({ "args": ["-c", "echo boom; exit 3"] }),
            None,
        )
        .unwrap();
    assert_eq!(result2.payload["status"], "failed");
    assert_eq!(result2.payload["exit_code"], 3);
    // 事件黑匣子：真实进程路径也全量入日志
    let evs = sink.0.lock().unwrap();
    let types: Vec<&str> = evs.iter().map(|e| e.event_type.as_str()).collect();
    assert!(types.contains(&"fabric.task_completed"));
    assert!(types.contains(&"fabric.task_failed"));
}

// ── tc_f8：stream 真实流式（逐行进度）+ cancel（kill 子进程）───────────
#[test]
fn tc_f8_stream_emits_progress_lines_and_cancel_kills() {
    // 流式：/bin/sh -c "printf 'a\nb\nc\n'" → 3 条 TaskProgress + 终局 TaskResult
    let sh = sh_adapter(&["shell.execute"]);
    let req = AgentMessage {
        id: "req_stream".into(),
        session_id: "sess_s".into(),
        sender: "dsh".into(),
        receiver: "sh-agent".into(),
        kind: MessageKind::TaskRequest,
        intent: Some("run_check".into()),
        payload: serde_json::json!({ "args": ["-c", "printf 'a\\nb\\nc\\n'"] }),
        context: Value::Null,
        capability: Some("shell.execute".into()),
        correlation_id: Some("c_stream".into()),
        causation_id: None,
        timestamp: 1,
    };
    let msgs = sh.stream(&req).unwrap();
    let kinds: Vec<&str> = msgs.iter().map(|m| m.kind.as_str()).collect();
    assert_eq!(
        kinds,
        vec![
            "task.progress",
            "task.progress",
            "task.progress",
            "task.result"
        ]
    );
    for (i, line) in ["a", "b", "c"].iter().enumerate() {
        assert_eq!(msgs[i].payload["progress"], *line);
    }
    assert_eq!(msgs[3].payload["status"], "completed");
    assert_eq!(msgs[3].payload["progress_count"], 3);
    // correlation 贯通
    assert!(
        msgs.iter()
            .all(|m| m.correlation_id.as_deref() == Some("c_stream"))
    );

    // cancel：长任务进程按 correlation_id 被杀 → 终局 failed
    let sh2 = Arc::new(sh_adapter(&["shell.execute"]));
    let req2 = AgentMessage {
        id: "req_cancel".into(),
        session_id: "sess_c".into(),
        sender: "dsh".into(),
        receiver: "sh-agent".into(),
        kind: MessageKind::TaskRequest,
        intent: Some("run_check".into()),
        payload: serde_json::json!({ "args": ["-c", "sleep 5; echo never"] }),
        context: Value::Null,
        capability: Some("shell.execute".into()),
        correlation_id: Some("c_cancel".into()),
        causation_id: None,
        timestamp: 1,
    };
    let handle = {
        let sh2 = sh2.clone();
        std::thread::spawn(move || {
            // 阻塞读（真实流式）；cancel 后管道关闭 → 提前返回
            sh2.stream(&req2).unwrap()
        })
    };
    std::thread::sleep(std::time::Duration::from_millis(300));
    // 取消（幂等：不存在的 correlation 也 Ok）
    assert!(sh2.cancel("c_cancel").is_ok());
    assert!(sh2.cancel("no-such").is_ok());
    let msgs = handle.join().expect("stream 线程应结束");
    assert!(
        msgs.iter().any(|m| m.kind == MessageKind::TaskResult),
        "cancel 后必须有终局结果"
    );
    let final_msg = msgs
        .iter()
        .rev()
        .find(|m| m.kind == MessageKind::TaskResult)
        .unwrap();
    assert_ne!(
        final_msg.payload["status"], "completed",
        "被取消的进程不得报 completed（kill 语义）"
    );
}

// ── CancelProbe：测试专用适配器 ───────────────────────────────────────────
// stream 启动时把 correlation_id 写入共享槽（供测试在 cancel 前拿到），并把子进程
// 句柄放入 running map（cancel 据此 kill）。cancel 按 correlation_id 移除并 kill。
// 复刻 ProcessAdapter 的真实 kill 语义，让 tc_f9 能在「执行中途」触发 cancel。
struct CancelProbe {
    corr: Arc<Mutex<Option<String>>>,
    running: Arc<Mutex<HashMap<String, Child>>>,
    /// 复刻 ProcessAdapter 的 killed 互斥设计（见 adapters.rs）。
    killed: Arc<Mutex<HashSet<String>>>,
}

impl AgentAdapter for CancelProbe {
    fn id(&self) -> &str {
        "probe"
    }
    fn connect(&self) -> Result<(), FabricError> {
        Ok(())
    }
    fn discover(&self) -> Vec<AgentCapability> {
        vec![AgentCapability {
            name: "shell.execute".into(),
            agent_id: "probe".into(),
            quality: 0.9,
            risk: 2,
        }]
    }
    fn send(&self, msg: &AgentMessage) -> Result<AgentMessage, FabricError> {
        // finish 走 stream，send 不被执行路径调用；返回固定完成结果以满足 trait
        Ok(AgentMessage {
            id: format!("m_{}", msg.id),
            session_id: msg.session_id.clone(),
            sender: "probe".into(),
            receiver: msg.sender.clone(),
            kind: MessageKind::TaskResult,
            intent: msg.intent.clone(),
            payload: serde_json::json!({ "status": "completed" }),
            context: Value::Null,
            capability: msg.capability.clone(),
            correlation_id: msg.correlation_id.clone(),
            causation_id: Some(msg.id.clone()),
            timestamp: msg.timestamp + 1,
        })
    }
    fn stream(&self, msg: &AgentMessage) -> Result<Vec<AgentMessage>, FabricError> {
        let args: Vec<String> = msg
            .payload
            .get("args")
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let mut child = Command::new("/bin/sh")
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| FabricError::Adapter(format!("进程启动失败: {e}")))?;
        let cid = msg.correlation_id.clone().unwrap_or_default();
        // 子进程句柄入 map（cancel 据此 kill）；stream 阻塞在 stdout 管道上
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| FabricError::Adapter("stdout 管道不可用".into()))?;
        self.running.lock().unwrap().insert(cid.clone(), child);
        // 子进程已 spawn 入 running map 后才暴露 correlation_id：
        // 测试拿到即代表可被 cancel 按 cid kill（消除「拿到 cid 但子进程尚未 spawn」竞态）
        if let Some(c) = &msg.correlation_id {
            *self.corr.lock().unwrap() = Some(c.clone());
        }
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let _ = line; // sleep 无输出；管道随进程结束/被 kill 关闭
        }
        // 管道关闭 → 移除并 wait。若 cancel 已先 remove+kill，此处 remove 为空 → exit=None；
        // killed 标记由 cancel 写入（与 running map 移除互斥），据以确定终态（复刻 ProcessAdapter）。
        let status = self
            .running
            .lock()
            .unwrap()
            .remove(&cid)
            .and_then(|mut c| c.wait().ok())
            .and_then(|s| s.code());
        let killed = self.killed.lock().unwrap().remove(&cid);
        let final_status = if killed {
            "killed"
        } else if status == Some(0) {
            "completed"
        } else {
            "failed"
        };
        Ok(vec![AgentMessage {
            id: format!("m_{}", msg.id),
            session_id: msg.session_id.clone(),
            sender: "probe".into(),
            receiver: msg.sender.clone(),
            kind: MessageKind::TaskResult,
            intent: msg.intent.clone(),
            payload: serde_json::json!({
                "status": final_status,
                "exit_code": status,
            }),
            context: Value::Null,
            capability: msg.capability.clone(),
            correlation_id: msg.correlation_id.clone(),
            causation_id: Some(msg.id.clone()),
            timestamp: msg.timestamp + 1,
        }])
    }
    fn cancel(&self, correlation_id: &str) -> Result<bool, FabricError> {
        if let Some(mut c) = self.running.lock().unwrap().remove(correlation_id) {
            self.killed
                .lock()
                .unwrap()
                .insert(correlation_id.to_string());
            c.kill()
                .map_err(|e| FabricError::Adapter(format!("kill 失败: {e}")))?;
            let _ = c.wait();
            Ok(true)
        } else {
            Ok(false) // 未在跑 = 幂等 no-op
        }
    }
}

// ── tc_f9：编排层 cancel——kill 子进程 + 取消标记集 → fabric.task_cancelled ──
// 证 T2 新增逻辑：Bridge::cancel 写标记集，finish 据此把终局事件选为
// `fabric.task_cancelled`（而非 failed），且子进程确被 kill（exit≠0）。
#[test]
fn tc_f9_cancel_kills_subprocess_and_emits_cancelled_event() {
    let sink = Arc::new(VecSink::default());
    let corr: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let running: Arc<Mutex<HashMap<String, Child>>> = Arc::new(Mutex::new(HashMap::new()));
    let killed: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    let probe = CancelProbe {
        corr: corr.clone(),
        running: running.clone(),
        killed: killed.clone(),
    };
    let mut r = FabricRouter::new();
    r.register_intent("run_slow", "shell.execute");
    r.register_agent("shell.execute", "probe", 1);
    let mut b = Bridge::new(r, Some(sink.clone()));
    b.connect_agent(Arc::new(probe)).unwrap();

    let fs = Arc::new(FabricService::new(b));
    let fs2 = fs.clone();
    // 提交长任务（sleep 30）于后台线程；submit 阻塞到终局（被 cancel 提前结束）
    let handle = std::thread::spawn(move || {
        fs2.submit(openaip_os::facade::FabricRequest {
            session_id: "sess-9".into(),
            sender: "tester".into(),
            intent: "run_slow".into(),
            payload: serde_json::json!({ "args": ["-c", "sleep 30"] }),
            project_id: None,
        })
    });
    // 轮询拿到 correlation_id（stream 启动即捕获）
    let cid = {
        let mut waited = 0;
        loop {
            if let Some(c) = corr.lock().unwrap().clone() {
                break c;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
            waited += 50;
            assert!(
                waited < 5000,
                "correlation_id 未在 5s 内捕获（stream 未启动）"
            );
        }
    };
    // 取消：kill 子进程 + 写取消标记集
    fs.cancel(&cid).expect("fabric_cancel 应成功");
    // 等待 submit 线程终局
    let res = handle
        .join()
        .expect("submit 线程应结束")
        .expect("submit 应成功");
    // 子进程被 cancel kill → 终态 status=killed（区别于正常 failed）
    assert_eq!(
        res.result.payload["status"], "killed",
        "被取消任务终态应为 killed（子进程被 cancel kill）"
    );
    // 关键：终局事件应为 fabric.task_cancelled（标记集命中），而非 fabric.task_failed
    let evs = sink.0.lock().unwrap();
    let types: Vec<&str> = evs.iter().map(|e| e.event_type.as_str()).collect();
    assert!(
        types.contains(&"fabric.task_cancelled"),
        "应发出 fabric.task_cancelled，实际: {types:?}"
    );
    assert!(
        !types.contains(&"fabric.task_failed"),
        "取消不应发 fabric.task_failed，实际: {types:?}"
    );
}

// ── tc_f10：P0 回归——stderr-heavy 子进程不得令 stream 死锁 ──────────────
// 子进程大量写 stderr（远超 OS 管道缓冲）时，若 stderr 管道被 piped 却无人读取，
// 子进程写满缓冲即阻塞 → 父进程读 stdout 等不到 EOF → 双向死锁，submit 永久挂起。
// 修复：stream 路径 stderr → Stdio::null（stream 语义本不捕获 stderr）。
// 修复前本测试会超时挂死（仅手动 cancel 能 unblock）。
#[test]
fn tc_f10_stream_does_not_deadlock_on_stderr_heavy_process() {
    let sink = Arc::new(VecSink::default());
    let mut r = FabricRouter::new();
    r.register_intent("run_loud", "shell.execute");
    r.register_agent("shell.execute", "sh-agent", 1);
    let mut b = Bridge::new(r, Some(sink.clone()));
    let sh = sh_adapter(&["shell.execute"]);
    b.connect_agent(Arc::new(sh)).unwrap();

    let fs = Arc::new(FabricService::new(b));
    let fs2 = fs.clone();
    // submit 阻塞到终局；后台线程跑，主线程用 recv_timeout 防死锁挂死 CI
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let res = fs2.submit(openaip_os::facade::FabricRequest {
            session_id: "sess-stderr".into(),
            sender: "tester".into(),
            intent: "run_loud".into(),
            // 写 ~1 万行 stderr（远超 macOS ~64KiB 管道缓冲），随后正常退出并打印 stdout
            payload: serde_json::json!({ "args": ["-c", "for i in $(seq 1 10000); do echo err-line-$i >&2; done; echo stdout-done"] }),
            project_id: None,
        });
        let _ = tx.send(res.map(|r| {
            r.result.payload["status"]
                .as_str()
                .unwrap_or("?")
                .to_string()
        }));
    });
    // 30s 内必须终局；否则即 stderr 管道死锁（P0 未修）
    let status = rx
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("stream 因 stderr 管道死锁挂死（P0 未修）")
        .expect("submit 应成功");
    assert_eq!(status, "completed", "stderr-heavy 任务应正常完成");
}
