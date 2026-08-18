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

    let sm = Arc::new(Mutex::new(SessionManager::new()));
    let s = sm
        .lock()
        .unwrap()
        .create(vec!["dsh".into(), "codex".into()]);
    let (msg, job) = b
        .submit(
            sm.clone(),
            &s.id,
            "dsh",
            "modify_code",
            serde_json::json!({ "repository": "demo", "task": "修复订单计算错误" }),
            None,
        )
        .unwrap();
    // T2b-2 异步契约：submit 立即返 running（执行经 execute）；correlation 必须贯通
    assert_eq!(msg.payload["status"], "running");
    assert!(msg.correlation_id.is_some(), "correlation 必须贯通");
    let result = b.execute(sm.clone(), job.unwrap()).unwrap();
    assert_eq!(result.kind, MessageKind::TaskResult);
    assert_eq!(result.payload["status"], "completed");
    assert_eq!(result.payload["files_changed"], 4);

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
    let guard = sm.lock().unwrap();
    let session = guard.get(&s.id).unwrap();
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

    let sm = Arc::new(Mutex::new(SessionManager::new()));
    let s = sm
        .lock()
        .unwrap()
        .create(vec!["dsh".into(), "codex".into()]);
    let (_msg, job) = b
        .submit(
            sm.clone(),
            &s.id,
            "dsh",
            "modify_code",
            serde_json::json!({ "object": "order-001", "task": "修复金额计算" }),
            None,
        )
        .unwrap();
    let result = b.execute(sm.clone(), job.unwrap()).unwrap();
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
use openaip_os::fabric::adapters::{ProcessAdapter, codex_adapter, dsh_adapter, shell_adapter};
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
        dsh_caps.iter().any(|c| c == "agent.task"),
        "dsh 通用 agent 任务入口应在: {dsh_caps:?}"
    );
    assert!(
        !dsh_caps.iter().any(|c| c == "shell.execute"),
        "B1：dsh 已撤下 shell.execute（执行归专用 shell-exec，不得假广告）: {dsh_caps:?}"
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

    let sm = Arc::new(Mutex::new(SessionManager::new()));
    let s = sm
        .lock()
        .unwrap()
        .create(vec!["dsh".into(), "sh-agent".into()]);
    let (_m1, job1) = b
        .submit(
            sm.clone(),
            &s.id,
            "dsh",
            "run_check",
            serde_json::json!({ "args": ["-c", "echo done-ok"] }),
            None,
        )
        .unwrap();
    let result = b.execute(sm.clone(), job1.unwrap()).unwrap();
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
    let (_m2, job2) = b
        .submit(
            sm.clone(),
            &s.id,
            "dsh",
            "run_check",
            serde_json::json!({ "args": ["-c", "echo boom; exit 3"] }),
            None,
        )
        .unwrap();
    let result2 = b.execute(sm.clone(), job2.unwrap()).unwrap();
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

// ── tc_f9：编排层 cancel——kill 子进程（adapter killed 互斥）→ fabric.task_cancelled ──
// 证 T2 新增逻辑：execute（后台 spawn_blocking）按 adapter 反馈的 killed 终态把终局事件
// 选为 `fabric.task_cancelled`（而非 failed），且子进程确被 kill。
// T2b-2 异步契约：submit 立即返 running + correlation_id，子进程在后台执行。
#[tokio::test(flavor = "multi_thread")]
async fn tc_f9_cancel_kills_subprocess_and_emits_cancelled_event() {
    let sink = Arc::new(VecSink::default());
    let corr: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let running: Arc<Mutex<HashMap<String, Child>>> = Arc::new(Mutex::new(HashMap::new()));
    let killed: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    let probe = CancelProbe {
        corr,
        running: running.clone(),
        killed,
    };
    let mut r = FabricRouter::new();
    r.register_intent("run_slow", "shell.execute");
    r.register_agent("shell.execute", "probe", 1);
    let mut b = Bridge::new(r, Some(sink.clone()));
    b.connect_agent(Arc::new(probe)).unwrap();

    // FabricService 持有运行时句柄：submit 立即返 running，子进程后台 spawn_blocking 执行。
    let fs = Arc::new(FabricService::new(b, tokio::runtime::Handle::current()));
    let sub = fs
        .submit(openaip_os::facade::FabricRequest {
            session_id: "sess-9".into(),
            sender: "tester".into(),
            intent: "run_slow".into(),
            payload: serde_json::json!({ "args": ["-c", "sleep 30"] }),
            project_id: None,
        })
        .expect("submit 应成功");
    assert_eq!(
        sub.result.payload["status"], "running",
        "异步契约：submit 立即返 running（不再阻塞到终局）"
    );
    let cid = sub.result.correlation_id.clone().unwrap();

    // 等子进程真正起来（running map 出现）再 cancel——避免异步竞态（cancel 早于 spawn）
    {
        let mut waited = 0;
        loop {
            if running.lock().unwrap().contains_key(&cid) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
            waited += 20;
            assert!(waited < 5000, "子进程未在 5s 内启动");
        }
    }
    // 取消：kill 子进程（adapter 层 killed 互斥——cancel 与 stream 以 running map 判据互斥）
    fs.cancel(&cid).expect("fabric_cancel 应成功");

    // 等待后台终局事件 fabric.task_cancelled（killed 终态）；且不得发 fabric.task_failed
    let mut cancelled = false;
    for _ in 0..200 {
        let (got_cancel, got_failed) = {
            let evs = sink.0.lock().unwrap();
            let types: Vec<&str> = evs.iter().map(|e| e.event_type.as_str()).collect();
            (
                types.contains(&"fabric.task_cancelled"),
                types.contains(&"fabric.task_failed"),
            )
        };
        if got_cancel {
            assert!(!got_failed, "取消不应发 fabric.task_failed");
            cancelled = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(cancelled, "应发出 fabric.task_cancelled（killed 终态）");
}

// ── tc_f10：P0 回归——stderr-heavy 子进程不得令 stream 死锁 ──────────────
// 子进程大量写 stderr（远超 OS 管道缓冲）时，若 stderr 管道被 piped 却无人读取，
// 子进程写满缓冲即阻塞 → 父进程读 stdout 等不到 EOF → 双向死锁，execute 永久挂起。
// 修复：stream 路径 stderr → Stdio::null（stream 语义本不捕获 stderr）。
// 修复前本测试会超时挂死（仅手动 cancel 能 unblock）。
// T2b-2：submit 立即返 running；执行语义（死锁面）在 execute——直接驱动 execute。
#[tokio::test(flavor = "multi_thread")]
async fn tc_f10_stream_does_not_deadlock_on_stderr_heavy_process() {
    let sink = Arc::new(VecSink::default());
    let mut r = FabricRouter::new();
    r.register_intent("run_loud", "shell.execute");
    r.register_agent("shell.execute", "sh-agent", 1);
    let mut b = Bridge::new(r, Some(sink.clone()));
    let sh = sh_adapter(&["shell.execute"]);
    b.connect_agent(Arc::new(sh)).unwrap();

    let sm = Arc::new(Mutex::new(SessionManager::new()));
    let s = sm
        .lock()
        .unwrap()
        .create(vec!["tester".into(), "sh-agent".into()]);
    let (_msg, job) = b
        .submit(
            sm.clone(),
            &s.id,
            "tester",
            "run_loud",
            // 写 ~1 万行 stderr（远超 macOS ~64KiB 管道缓冲），随后正常退出并打印 stdout
            serde_json::json!({ "args": ["-c", "for i in $(seq 1 10000); do echo err-line-$i >&2; done; echo stdout-done"] }),
            None,
        )
        .unwrap();
    // execute 同步阻塞到终局；后台线程跑，主线程用 recv_timeout 防死锁挂死 CI
    let job = job.expect("submit 应返回执行任务包");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let res = b
            .execute(sm, job)
            .map(|r| r.payload["status"].as_str().unwrap_or("?").to_string());
        let _ = tx.send(res);
    });
    // 30s 内必须终局；否则即 stderr 管道死锁（P0 未修）
    let status = rx
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("stream 因 stderr 管道死锁挂死（P0 未修）")
        .expect("execute 应成功");
    assert_eq!(status, "completed", "stderr-heavy 任务应正常完成");
}

// ── StreamFailProbe：stream 直接返回 Err 的探针适配器（H1 验证用）────────
// 复刻「运行时命令不可用」真实失败：adapter.stream 启动子进程即失败 → Err。
// 验证 H1：run_in_background 不得吞错，必须发 fabric.task_failed + 会话终态，
// 否则 correlation 永久悬挂、CLI 误导「超时未终局」。
struct StreamFailProbe;

impl AgentAdapter for StreamFailProbe {
    fn id(&self) -> &str {
        "fail-probe"
    }
    fn connect(&self) -> Result<(), FabricError> {
        Ok(())
    }
    fn discover(&self) -> Vec<AgentCapability> {
        vec![AgentCapability {
            name: "shell.execute".into(),
            agent_id: "fail-probe".into(),
            quality: 0.9,
            risk: 2,
        }]
    }
    fn send(&self, msg: &AgentMessage) -> Result<AgentMessage, FabricError> {
        Ok(AgentMessage {
            id: format!("m_{}", msg.id),
            session_id: msg.session_id.clone(),
            sender: "fail-probe".into(),
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
    fn stream(&self, _msg: &AgentMessage) -> Result<Vec<AgentMessage>, FabricError> {
        Err(FabricError::Adapter("子进程启动失败：命令不可用".into()))
    }
    fn cancel(&self, _correlation_id: &str) -> Result<bool, FabricError> {
        Ok(false) // 未在跑 = 幂等 no-op
    }
}

// ── tc_f11：H1 回归——stream 失败必须发 fabric.task_failed（不再被吞）────
// 修复前：execute 在 adapter.stream 返回 Err 时先于发终局事件就 return Err，
// run_in_background 把错误 `let _ = e` 吞掉 → 无 fabric.task_failed、会话无终态、
// correlation 永久悬挂。修复后：Err 分支先发 fabric.task_failed（带错误文本）+
// 会话 append，再返 Err。
#[tokio::test(flavor = "multi_thread")]
async fn tc_f11_stream_failure_emits_task_failed_not_swallowed() {
    let sink = Arc::new(VecSink::default());
    let mut r = FabricRouter::new();
    r.register_intent("run_fail", "shell.execute");
    r.register_agent("shell.execute", "fail-probe", 1);
    let mut b = Bridge::new(r, Some(sink.clone()));
    b.connect_agent(Arc::new(StreamFailProbe)).unwrap();

    let fs = Arc::new(FabricService::new(b, tokio::runtime::Handle::current()));
    let sub = fs
        .submit(openaip_os::facade::FabricRequest {
            session_id: "sess-11".into(),
            sender: "tester".into(),
            intent: "run_fail".into(),
            payload: serde_json::json!({ "args": ["-c", "nope"] }),
            project_id: None,
        })
        .expect("submit 应成功");
    assert_eq!(
        sub.result.payload["status"], "running",
        "异步契约：submit 立即返 running"
    );

    // 等后台 execute 跑完：stream 返回 Err → 必须发 fabric.task_failed（H1 修复前被吞噬）
    let mut failed = false;
    for _ in 0..250 {
        {
            let evs = sink.0.lock().unwrap();
            let types: Vec<&str> = evs.iter().map(|e| e.event_type.as_str()).collect();
            if types.contains(&"fabric.task_failed") {
                // 错误文本随事件传播，便于 CLI 诊断（不再静默悬挂）
                assert!(
                    evs.iter().any(|e| e.event_type == "fabric.task_failed"
                        && e.input
                            .as_ref()
                            .map(|i| i.to_string().contains("命令不可用"))
                            .unwrap_or(false)),
                    "fabric.task_failed 应携带错误文本"
                );
                assert!(
                    !types.contains(&"fabric.task_completed"),
                    "失败不得发 fabric.task_completed"
                );
                failed = true;
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        failed,
        "stream 失败必须发 fabric.task_failed（H1：不再被 run_in_background 吞噬）"
    );
}

// ── tc_f12：H2 回归——early-cancel 不等 running map 即取消也生效 ─────────
// 复刻 submit 返 running 与子进程进 running map 之间的窗口：修复前此间到达的
// cancel 命中 0（adapter 还没 spawn 子进程）却无任何反馈，任务照跑完成 →
// 「取消无效」。修复后：Bridge::cancel 写 cancel_requested 标记集，execute 在
// spawn 子进程前检查命中 → 直接发 fabric.task_cancelled。本测试刻意不等待
// running map（与 tc_f9 相反），专打 early 窗口。
#[tokio::test(flavor = "multi_thread")]
async fn tc_f12_early_cancel_without_waiting_running_map_is_effective() {
    let sink = Arc::new(VecSink::default());
    let corr: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let running: Arc<Mutex<HashMap<String, Child>>> = Arc::new(Mutex::new(HashMap::new()));
    let killed: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    let probe = CancelProbe {
        corr,
        running: running.clone(),
        killed,
    };
    let mut r = FabricRouter::new();
    r.register_intent("run_slow2", "shell.execute");
    r.register_agent("shell.execute", "probe", 1);
    let mut b = Bridge::new(r, Some(sink.clone()));
    b.connect_agent(Arc::new(probe)).unwrap();

    let fs = Arc::new(FabricService::new(b, tokio::runtime::Handle::current()));
    let sub = fs
        .submit(openaip_os::facade::FabricRequest {
            session_id: "sess-12".into(),
            sender: "tester".into(),
            intent: "run_slow2".into(),
            payload: serde_json::json!({ "args": ["-c", "sleep 30"] }),
            project_id: None,
        })
        .expect("submit 应成功");
    assert_eq!(
        sub.result.payload["status"], "running",
        "异步契约：submit 立即返 running"
    );
    let cid = sub.result.correlation_id.clone().unwrap();

    // H2：不等 running map 出现即 cancel（early 窗口）。无论走 early（cancel_requested
    // 命中直接 cancelled）还是 late（adapter killed mutex kill）路径，都必须发出
    // fabric.task_cancelled；且不得误报 fabric.task_completed。
    fs.cancel(&cid).expect("fabric_cancel 应成功");

    let mut cancelled = false;
    for _ in 0..400 {
        {
            let evs = sink.0.lock().unwrap();
            let types: Vec<&str> = evs.iter().map(|e| e.event_type.as_str()).collect();
            if types.contains(&"fabric.task_cancelled") {
                assert!(
                    !types.contains(&"fabric.task_completed"),
                    "early-cancel 不得误报完成"
                );
                cancelled = true;
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        cancelled,
        "不等 running map 的 early-cancel 必须生效（发 fabric.task_cancelled）"
    );
}

// ── tc_f13：S2 孤儿后台 job 回收——shutdown 杀掉运行中的子进程 ──────────
//
// 覆盖 S2 的核心逻辑：`ProcessAdapter::shutdown` 在调用时 drain `running` 中
// 所有子进程并 kill（孤儿后台 job 回收），使服务优雅关闭时不会遗留仍在跑的
// 后台任务。与 `cancel`（按 correlation_id 精准 kill）不同，`shutdown` 是
// 无差别回收全部运行中子进程——这是优雅关闭路径的末端动作。
//
// 确定性处理：用 marker 文件作为「子进程已 spawn 且正在运行」的就绪信号，
// 消除「shutdown 早于子进程进 running map」的竞态（比纯定时 sleep 更稳）；
// 用 `mpsc::recv_timeout` 做 fail-fast 兜底——若 shutdown 未生效，stream 会
// 阻塞满 30s，测试应在 5s 内判定失败而非挂死。
#[test]
fn tc_f13_shutdown_kills_running_child() {
    let marker = "/tmp/oaip_tc_f13_marker";
    let _ = std::fs::remove_file(marker);

    // 真实 /bin/sh（shell 模式）适配器；跑一个长 sleep 子进程，进入 running map。
    let sh = Arc::new(shell_adapter(None));
    let req = AgentMessage {
        id: "req_shutdown".into(),
        session_id: "sess_sh".into(),
        sender: "dsh".into(),
        receiver: "shell".into(),
        kind: MessageKind::TaskRequest,
        intent: Some("run_shell".into()),
        payload: serde_json::json!({ "args": ["-c", format!("touch {marker}; sleep 30")] }),
        context: Value::Null,
        capability: Some("shell.execute".into()),
        correlation_id: Some("c_shutdown".into()),
        causation_id: None,
        timestamp: 1,
    };
    // stream 同步阻塞（真实流式读 stdout），另起 OS 线程跑，避免阻塞测试线程。
    let sh_clone = sh.clone();
    let stream_handle = std::thread::spawn(move || sh_clone.stream(&req).unwrap());

    // 轮询 marker 最多 5s，确认子进程已启动并在 running map 中（就绪后再 shutdown）。
    let start = std::time::Instant::now();
    while !std::path::Path::new(marker).exists()
        && start.elapsed() < std::time::Duration::from_secs(5)
    {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        std::path::Path::new(marker).exists(),
        "子进程应在 5s 内启动（touch marker）；否则 shutdown 测试前提不成立"
    );

    // S2 核心：shutdown 无差别 drain running 子进程并 kill —— 孤儿 sleep 必须被回收，
    // 而非跑满 30s 自然结束（否则服务关闭会遗留孤儿后台进程）。
    sh.shutdown();

    // shutdown 生效 → 子进程被 kill → stdout 管道关闭 → stream 读到 EOF 提前返回。
    // 用 mpsc + recv_timeout 做 fail-fast：若未生效，stream 阻塞满 30s，测试 5s 判失败。
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let r = stream_handle.join().expect("stream 线程应结束");
        let _ = tx.send(r);
    });
    let msgs = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("shutdown 应在 5s 内杀掉运行中的子进程（孤儿回收），而非等满 30s");

    // 被 kill 的进程退出码非 0（或 None）→ status 不是 completed（与 cancel 同语义）；
    // 关键回归信号：shutdown 必须阻止子进程跑满 30s 自然完成。
    let final_msg = msgs
        .iter()
        .rev()
        .find(|m| m.kind == MessageKind::TaskResult)
        .expect("shutdown 杀掉子进程后应有终局结果");
    assert_ne!(
        final_msg.payload["status"], "completed",
        "shutdown 必须杀掉运行中的子进程（不得让它跑满 30s 报 completed）"
    );

    let _ = std::fs::remove_file(marker);
}
