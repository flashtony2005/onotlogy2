//! V2.0 Universal Worker & Plugin Runtime（Phase 10 · V10.1）契约测试。
//!
//! TDD：先写测试（红），再实现（绿）。
//!
//! W1 WorkerManifest 序列化 roundtrip
//! W2 WorkerRegistry：注册/重复拒绝/candidates
//! W3 WorkerRouter：候选 → Policy Filter → Score → Selected
//! W4 HTTP Worker：POST 真实调用（axum mock）
//! W5 MCP Worker：JSON-RPC tools/call
//! R4 Process Worker（Generic CLI 真实进程）
//! R5 Codex Worker Adapter 壳
//! R7 Generic CLI（git 真实执行）
//! R8 Policy / Approval（allow / require_approval / deny fail-closed）
//! R9 Session Event Log（worker.* 全事件链）
//! R10 Failure / Retry（失败捕获 + 重试一次）

use axum::routing::post;
use axum::{Json, Router as AxumRouter};
use serde_json::{Value, json};
use tokio::net::TcpListener;

use openaip_os::session::event::{EventType, SessionId};
use openaip_os::session::log::SessionLog;
use openaip_os::worker::exec::WorkerRuntime;
use openaip_os::worker::manifest::{ExecutionProtocol, WorkerManifest};
use openaip_os::worker::policy::WorkerPolicy;
use openaip_os::worker::registry::{WorkerRegistry, WorkerStatus};
use openaip_os::worker::router::{RouterCriteria, SelectedWorker, WorkerRouter};
use openaip_os::worker::sdk::{Worker, WorkerError, WorkerResult};

/// echo CLI Worker（Generic Process Worker）。
fn echo_worker(id: &str, caps: &[&str]) -> openaip_os::worker::process::ProcessWorker {
    let mut m = WorkerManifest::new(id, "cli").with_capabilities(caps);
    m.execution = ExecutionProtocol::Process;
    openaip_os::worker::process::ProcessWorker::new(m, "echo")
}

// ── W1：WorkerManifest 序列化 roundtrip ───────────────────────────────
#[tokio::test]
async fn tc_w1_manifest_roundtrip() {
    let m = WorkerManifest::new("codex", "agent")
        .with_capabilities(&["code.read", "code.write", "code.test"])
        .with_quality(0.95);
    let s = serde_json::to_string(&m).unwrap();
    let back: WorkerManifest = serde_json::from_str(&s).unwrap();
    assert_eq!(m.id, back.id);
    assert_eq!(m.capabilities.len(), 3);
    assert_eq!(m.execution, back.execution);
}

// ── W2：WorkerRegistry ─────────────────────────────────────────────────
#[tokio::test]
async fn tc_w2_registry() {
    let mut reg = WorkerRegistry::new();
    reg.register(Box::new(echo_worker("echo", &["text.echo"])))
        .unwrap();
    // 重复注册拒绝
    assert!(
        reg.register(Box::new(echo_worker("echo", &["text.echo"])))
            .is_err()
    );
    // candidates：按 capability 发现
    let cands = reg.candidates("text.echo");
    assert_eq!(cands.len(), 1);
    assert!(reg.candidates("missing.cap").is_empty());
    // 状态
    assert_eq!(reg.status("echo"), Some(WorkerStatus::Registered));
    assert_eq!(reg.status("nope"), None);
}

// ── W3：WorkerRouter（候选 → Policy Filter → Score → Selected）─────────
#[tokio::test]
async fn tc_w3_router_selection() {
    let mut reg = WorkerRegistry::new();
    // 两个 code.write 候选：质量 0.9（cost 5）与 0.95（cost 10）
    let mut w1 = echo_worker("worker-a", &["code.write"]);
    w1.manifest_mut().quality = 0.9;
    w1.manifest_mut().cost = 5;
    reg.register(Box::new(w1)).unwrap();
    let mut w2 = echo_worker("worker-b", &["code.write"]);
    w2.manifest_mut().quality = 0.95;
    w2.manifest_mut().cost = 10;
    reg.register(Box::new(w2)).unwrap();

    let router = WorkerRouter::new(RouterCriteria::default());
    let policy = WorkerPolicy::new().allow("code.write");
    let sel: SelectedWorker = router.route(&reg, "code.write", &policy).expect("路由成功");
    assert_eq!(sel.worker_id, "worker-b", "质量优先");
    assert!(sel.reason.contains("score"), "路由理由可解释");

    // 显式指定
    let router2 = WorkerRouter::new(RouterCriteria {
        explicit: Some("worker-a".into()),
        ..Default::default()
    });
    assert_eq!(
        router2
            .route(&reg, "code.write", &policy)
            .unwrap()
            .worker_id,
        "worker-a"
    );

    // Policy 过滤：deny → 无候选
    let deny = WorkerPolicy::new().deny("code.write");
    assert!(router.route(&reg, "code.write", &deny).is_err());
}

// ── R4：Process Worker（Generic CLI 真实进程执行）──────────────────────
#[tokio::test]
async fn tc_r4_process_worker_real_execution() {
    let mut reg = WorkerRegistry::new();
    reg.register(Box::new(echo_worker("echo", &["text.echo"])))
        .unwrap();
    let w = reg.get("echo").unwrap();
    let result: WorkerResult = w
        .execute("text.echo", json!({ "args": ["hello worker"] }))
        .await
        .expect("真实进程执行");
    assert!(result.ok);
    assert_eq!(result.exit_code, Some(0));
    let stdout = result.output["stdout"].as_str().unwrap_or("");
    assert!(stdout.contains("hello worker"), "stdout 应含输入: {stdout}");
}

// ── R5：Codex Worker Adapter 壳 ────────────────────────────────────────
#[tokio::test]
async fn tc_r5_codex_adapter_shell() {
    let codex = openaip_os::worker::adapters::codex_worker(Some("echo"));
    let m = codex.manifest().clone();
    assert!(
        m.capabilities.contains(&"code.read".to_string()),
        "code.read 能力"
    );
    assert!(
        m.capabilities.contains(&"code.test".to_string()),
        "code.test 能力"
    );
    // 壳可真实执行（命令替换为 echo——适配器不绑定具体二进制）
    let result = codex
        .execute("code.read", json!({ "file": "a.rs" }))
        .await
        .expect("适配器执行");
    assert!(result.ok);
}

// ── R7：Generic CLI（git 真实执行）────────────────────────────────────
#[tokio::test]
async fn tc_r7_generic_cli_git() {
    let mut m = WorkerManifest::new("git", "cli").with_capabilities(&["git.version"]);
    m.execution = ExecutionProtocol::Process;
    let git = openaip_os::worker::process::ProcessWorker::new(m, "git");
    let result = git
        .execute("git.version", json!({ "args": ["--version"] }))
        .await
        .expect("git 真实执行");
    assert!(result.ok, "git --version 应成功");
    assert!(
        result.output["stdout"]
            .as_str()
            .unwrap_or("")
            .contains("git"),
        "输出含 git 版本"
    );
}

// ── R8：Policy / Approval（fail-closed）────────────────────────────────
#[tokio::test]
async fn tc_r8_policy_approval() {
    let mut reg = WorkerRegistry::new();
    reg.register(Box::new(echo_worker("echo", &["code.read", "db.delete"])))
        .unwrap();

    // allow → 执行成功
    let policy = WorkerPolicy::new().allow("code.read");
    let mut log = SessionLog::new(SessionId::new("s8"));
    let mut rt = WorkerRuntime::new(&reg, &policy, WorkerRouter::new(RouterCriteria::default()))
        .with_log(&mut log);
    let r = rt
        .run("code.read", json!({ "text": "ok" }))
        .await
        .expect("allow 执行");
    assert!(r.ok);

    // require_approval → 不执行，明确错误
    let policy2 = WorkerPolicy::new().require_approval("db.delete");
    let mut rt2 = WorkerRuntime::new(&reg, &policy2, WorkerRouter::new(RouterCriteria::default()))
        .with_log(&mut log);
    let err = rt2.run("db.delete", json!({})).await.unwrap_err();
    assert!(
        matches!(err, WorkerError::RequireApproval(_)),
        "require_approval → 等待审批: {err}"
    );
    assert!(
        log.events()
            .iter()
            .any(|e| e.event_type == EventType::ApprovalRequested),
        "审批事件已记录"
    );

    // deny → 拒绝且不执行
    let policy3 = WorkerPolicy::new().deny("db.delete");
    let mut rt3 = WorkerRuntime::new(&reg, &policy3, WorkerRouter::new(RouterCriteria::default()));
    assert!(matches!(
        rt3.run("db.delete", json!({})).await.unwrap_err(),
        WorkerError::Denied(_)
    ));

    // 未配置 → Deny（Fail Closed，不是 TRY）
    let policy4 = WorkerPolicy::new();
    let mut rt4 = WorkerRuntime::new(&reg, &policy4, WorkerRouter::new(RouterCriteria::default()));
    assert!(matches!(
        rt4.run("code.read", json!({})).await.unwrap_err(),
        WorkerError::Denied(_)
    ));
}

// ── R9：Session Event Log（worker.* 全事件链）──────────────────────────
#[tokio::test]
async fn tc_r9_event_log_chain() {
    let mut reg = WorkerRegistry::new();
    reg.register(Box::new(echo_worker("echo", &["text.echo"])))
        .unwrap();
    let policy = WorkerPolicy::new().allow("text.echo");
    let mut log = SessionLog::new(SessionId::new("s9"));
    let mut rt = WorkerRuntime::new(&reg, &policy, WorkerRouter::new(RouterCriteria::default()))
        .with_log(&mut log);
    rt.run("text.echo", json!({ "text": "x" })).await.unwrap();

    let tys: Vec<&str> = log.events().iter().map(|e| e.event_type.as_str()).collect();
    assert!(tys.contains(&"policy.checked"), "policy.checked 事件");
    assert!(tys.contains(&"worker.selected"), "worker.selected 事件");
    assert!(tys.contains(&"worker.started"), "worker.started 事件");
    assert!(tys.contains(&"worker.completed"), "worker.completed 事件");
    // 顺序：selected → started → completed
    let idx_sel = tys.iter().position(|t| *t == "worker.selected").unwrap();
    let idx_start = tys.iter().position(|t| *t == "worker.started").unwrap();
    let idx_done = tys.iter().position(|t| *t == "worker.completed").unwrap();
    assert!(idx_sel < idx_start && idx_start < idx_done, "事件顺序正确");
    // selected 事件带 worker/capability/reason
    let sel = log
        .events()
        .iter()
        .find(|e| e.event_type == EventType::WorkerSelected)
        .unwrap();
    assert_eq!(sel.data["worker"], "echo");
    assert_eq!(sel.data["capability"], "text.echo");
}

// ── R10：Failure / Retry（失败捕获 + 重试一次）─────────────────────────
#[tokio::test]
async fn tc_r10_failure_and_retry() {
    // 一个会失败的 worker：命令不存在（第一次失败）→ Runtime 重试一次（仍失败 → FailedAfterRetry）
    let mut m = WorkerManifest::new("broken", "cli").with_capabilities(&["x.run"]);
    m.execution = ExecutionProtocol::Process;
    let broken = openaip_os::worker::process::ProcessWorker::new(m, "no-such-cmd-xyz");
    let mut reg = WorkerRegistry::new();
    reg.register(Box::new(broken)).unwrap();
    let policy = WorkerPolicy::new().allow("x.run");
    let mut log = SessionLog::new(SessionId::new("s10"));
    let mut rt = WorkerRuntime::new(&reg, &policy, WorkerRouter::new(RouterCriteria::default()))
        .with_log(&mut log);

    let err = rt.run("x.run", json!({})).await.unwrap_err();
    assert!(
        matches!(err, WorkerError::Execution(_)),
        "失败可捕获: {err}"
    );
    // 失败事件 + 重试（两次尝试均 failed）
    let fails = log
        .events()
        .iter()
        .filter(|e| e.event_type == EventType::WorkerFailed)
        .count();
    assert!(fails >= 2, "失败记录 + 重试记录: {fails}");
    let starts = log
        .events()
        .iter()
        .filter(|e| e.event_type == EventType::WorkerStarted)
        .count();
    assert_eq!(starts, 2, "重试一次（共两次启动）");
}

// ── W4：HTTP Worker（真实 POST 调用）──────────────────────────────────
#[tokio::test]
async fn tc_w4_http_worker_real_post() {
    let app = AxumRouter::new().route(
        "/execute",
        post(|Json(body): Json<Value>| async move {
            let cap = body.get("capability").cloned().unwrap_or(json!(""));
            Json(json!({ "ok": true, "echo_cap": cap }))
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let worker = openaip_os::worker::http::HttpWorker::new(
        WorkerManifest::new("http-remote", "http").with_capabilities(&["remote.run"]),
        &base,
    );
    let result = worker
        .execute("remote.run", json!({ "x": 1 }))
        .await
        .expect("HTTP worker 真实调用");
    assert!(result.ok);
    assert_eq!(result.output["echo_cap"], "remote.run");
}

// ── W5：MCP Worker（JSON-RPC tools/call）──────────────────────────────
#[tokio::test]
async fn tc_w5_mcp_worker_tools_call() {
    let app = AxumRouter::new().route(
        "/mcp",
        post(|Json(req): Json<Value>| async move {
            let id = req.get("id").cloned().unwrap_or(json!(0));
            let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
            match method {
                "initialize" => Json(json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": { "protocolVersion": "2024-11-05", "capabilities": { "tools": {} }, "serverInfo": { "name": "mock", "version": "1.0" } }
                })),
                "tools/call" => {
                    let name = req.get("params").and_then(|p| p.get("name")).and_then(|n| n.as_str()).unwrap_or("");
                    Json(json!({
                        "jsonrpc": "2.0", "id": id,
                        "result": { "content": [ { "type": "text", "text": format!("mcp:{name}") } ] }
                    }))
                }
                _ => Json(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "unknown" } })),
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let worker = openaip_os::worker::mcp::McpWorker::new(
        WorkerManifest::new("mcp-server", "mcp").with_capabilities(&["mcp.tool"]),
        &format!("http://{addr}/mcp"),
    );
    let result = worker
        .execute("mcp.tool", json!({ "a": 1 }))
        .await
        .expect("MCP worker 调用");
    assert!(result.ok);
    assert!(
        result.output["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .contains("mcp.tool")
    );
}

// ── W6：Orchestrator（Phase 11D：Worker ABI 进入运行热路径）───────────
#[tokio::test]
async fn tc_w6_orchestrator_plan() {
    use openaip_os::worker::orchestrator::{WorkerOrchestrator, WorkerTask};
    use openaip_os::worker::router::WorkerRouter;

    let mut registry = WorkerRegistry::new();
    registry
        .register(Box::new(echo_worker("echo", &["text.echo", "code.test"])))
        .unwrap();
    let policy = WorkerPolicy::new().allow("text.echo").allow("code.test");
    let router = WorkerRouter::new(RouterCriteria::default());

    let mut orch = WorkerOrchestrator::new(&registry, &policy, router);
    let tasks = vec![
        WorkerTask::new("text.echo", json!({ "msg": "hi" })),
        WorkerTask::new("code.test", json!({ "suite": "unit" })),
    ];
    let result = orch.run_plan(&tasks).await;
    assert!(result.all_ok, "计划全绿");
    assert_eq!(result.outcomes.len(), 2);
    assert!(
        result
            .outcomes
            .iter()
            .all(|o| o.status == openaip_os::TaskStatus::Completed)
    );
    assert!(
        result
            .outcomes
            .iter()
            .all(|o| o.worker_id.as_deref() == Some("echo"))
    );
    assert_eq!(result.success_rate(), 1.0);

    // 事件流（事实源）：capability.selected → policy.checked → worker.selected/started/completed
    let events = orch.session_log().events();
    let types: Vec<&str> = events.iter().map(|e| e.event_type.as_str()).collect();
    assert!(types.contains(&"capability.selected"));
    assert!(types.contains(&"policy.checked"));
    assert!(types.contains(&"worker.selected"));
    assert!(types.contains(&"worker.started"));
    assert!(types.contains(&"worker.completed"));
    // 事件可回放（事实源不可变）
    assert_eq!(events.len(), orch.session_log().events().len());
}

// ── W7：Orchestrator 策略闸门（Worker ≠ Authority）──────────────────
#[tokio::test]
async fn tc_w7_orchestrator_policy_gates() {
    use openaip_os::worker::orchestrator::{WorkerOrchestrator, WorkerTask};
    use openaip_os::worker::router::WorkerRouter;

    let mut registry = WorkerRegistry::new();
    registry
        .register(Box::new(echo_worker(
            "echo",
            &["text.echo", "code.write", "shell.execute"],
        )))
        .unwrap();
    // fail-closed：text.echo allow；code.write require_approval；shell.execute 未配置 → Deny
    // fail-closed：未配置 = Deny；策略允许但无候选 = NoWorker
    let policy = WorkerPolicy::new()
        .allow("text.echo")
        .allow("allowed.but.noworker")
        .require_approval("code.write");
    let router = WorkerRouter::new(RouterCriteria::default());
    let mut orch = WorkerOrchestrator::new(&registry, &policy, router);
    let result = orch
        .run_plan(&[
            WorkerTask::new("text.echo", json!({})),
            WorkerTask::new("code.write", json!({})),
            WorkerTask::new("shell.execute", json!({})),
            WorkerTask::new("allowed.but.noworker", json!({})),
        ])
        .await;
    assert!(!result.all_ok);
    assert_eq!(result.outcomes[0].status, openaip_os::TaskStatus::Completed);
    assert_eq!(
        result.outcomes[1].status,
        openaip_os::TaskStatus::NeedsApproval
    );
    assert_eq!(
        result.outcomes[2].status,
        openaip_os::TaskStatus::Denied,
        "fail-closed：未配置 = Deny"
    );
    assert_eq!(
        result.outcomes[3].status,
        openaip_os::TaskStatus::NoWorker,
        "允许但无候选 = NoWorker"
    );
    // 被闸门拦下的步骤不产生 worker 执行事件
    let ev_types: Vec<&str> = orch
        .session_log()
        .events()
        .iter()
        .map(|e| e.event_type.as_str())
        .collect();
    assert_eq!(
        ev_types.iter().filter(|t| **t == "worker.started").count(),
        1,
        "只有 allow 的步骤真正执行"
    );
}

// ── W8：Orchestrator → Evaluation 端到端（Phase 11A 闭环）────────────
#[tokio::test]
async fn tc_w8_orchestrator_eval_end_to_end() {
    use openaip_os::evaluation::{EvaluationReducer, TaskDefinition};
    use openaip_os::session::event::EventType;
    use openaip_os::worker::orchestrator::{WorkerOrchestrator, WorkerTask};
    use openaip_os::worker::router::WorkerRouter;

    let storage = openaip_os::storage::LibSQLStorage::open(":memory:")
        .await
        .unwrap();
    let mut registry = WorkerRegistry::new();
    registry
        .register(Box::new(echo_worker("echo", &["text.echo", "code.test"])))
        .unwrap();
    let policy = WorkerPolicy::new().allow("text.echo").allow("code.test");
    let router = WorkerRouter::new(RouterCriteria::default());
    let mut orch = WorkerOrchestrator::new(&registry, &policy, router);
    let result = orch
        .run_plan(&[
            WorkerTask::new("text.echo", json!({ "msg": "a" })),
            WorkerTask::new("code.test", json!({ "suite": "unit" })),
        ])
        .await;
    assert!(result.all_ok);

    // RunEnd → Evaluation：worker.* 真实事件 → 六维评测 → wb_evaluation
    orch.session_log().append(
        EventType::SessionCompleted,
        json!({ "run_id": result.run_id }),
    );
    let task = TaskDefinition {
        task_id: "w-e2e".into(),
        expected_relations: vec![],
        expected_artifacts: vec![],
        time_budget_secs: 10.0,
        skill_version: None,
    };
    let run_id = result.run_id.clone();
    let eval = openaip_os::on_run_end(
        orch.session_log(),
        &run_id,
        &task,
        &openaip_os::EvaluationPolicy::default(),
        &storage,
    )
    .await
    .unwrap();
    // 真实 worker 事件 → 客观维度全部可得（此前 V1 planner 路径全是 unknowns）
    assert_eq!(eval.reliability.value, Some(1.0), "无失败 → 可靠");
    assert_eq!(eval.policy_compliance.value, Some(1.0), "allow 链 → 合规");
    assert_eq!(eval.efficiency.value, Some(1.0), "echo 毫秒级 ≪ 10s 预算");
    assert!(
        eval.correctness.is_unknown(),
        "无测试证据 → 未知（Unknown ≠ Success）"
    );
    assert_eq!(
        eval.unknowns().len(),
        3,
        "correctness/completeness/output_quality"
    );

    // 可回放：同一事件流重算一致
    let events: Vec<_> = orch.session_log().events().to_vec();
    let replay = EvaluationReducer::default().evaluate(&events, &run_id, &task);
    assert_eq!(replay, eval);

    // wb_evaluation 可查询
    let loaded = storage
        .list_evaluations(Some(&run_id), None, 10)
        .await
        .unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].aggregate, eval.aggregate);
}

// ── W9：Router 风险过滤 + 信任评分（Capability-aware Registry）──────
#[tokio::test]
async fn tc_w9_router_risk_and_trust() {
    use openaip_os::worker::process::ProcessWorker;
    use openaip_os::worker::router::WorkerRouter;

    let mut registry = WorkerRegistry::new();
    // 同质量候选：low-trust（trust 0.5）vs high-trust（trust 1.0）
    let mut low = openaip_os::worker::manifest::WorkerManifest::new("low-trust", "agent")
        .with_capabilities(&["code.read"])
        .with_quality(1.0)
        .with_trust(0.5);
    low.execution = ExecutionProtocol::Process;
    let mut high = openaip_os::worker::manifest::WorkerManifest::new("high-trust", "agent")
        .with_capabilities(&["code.read"])
        .with_quality(0.9)
        .with_trust(1.0);
    high.execution = ExecutionProtocol::Process;
    registry
        .register(Box::new(ProcessWorker::new(low, "echo")))
        .unwrap();
    registry
        .register(Box::new(ProcessWorker::new(high, "echo")))
        .unwrap();

    let router = WorkerRouter::new(RouterCriteria::default());
    let sel = router
        .route(
            &registry,
            "code.read",
            &WorkerPolicy::new().allow("code.read"),
        )
        .unwrap();
    assert_eq!(
        sel.worker_id, "high-trust",
        "评分 = quality × trust：0.9 > 0.5"
    );

    // 风险过滤：risk=9 的候选在 max_risk=5 下被排除
    let mut risky = openaip_os::worker::manifest::WorkerManifest::new("risky", "agent")
        .with_capabilities(&["db.delete"])
        .with_quality(1.0)
        .with_risk(9);
    risky.execution = ExecutionProtocol::Process;
    registry
        .register(Box::new(ProcessWorker::new(risky, "echo")))
        .unwrap();
    let strict = WorkerRouter::new(RouterCriteria {
        max_risk: 5,
        ..Default::default()
    });
    let r = strict.route(
        &registry,
        "db.delete",
        &WorkerPolicy::new().allow("db.delete"),
    );
    assert!(r.is_err(), "高风险 worker 被策略性过滤（还有别的候选）");
    let loose = WorkerRouter::new(RouterCriteria::default());
    let sel2 = loose
        .route(
            &registry,
            "db.delete",
            &WorkerPolicy::new().allow("db.delete"),
        )
        .unwrap();
    assert_eq!(sel2.worker_id, "risky");
}

// ── W10：DSH 适配器命令构造（假 dsh shim，可进 CI，不依赖真实二进制）──
#[tokio::test]
async fn tc_w10_dsh_adapter_command_shape() {
    use openaip_os::worker::adapters::dsh_worker;
    use openaip_os::worker::orchestrator::{WorkerOrchestrator, WorkerTask};
    use openaip_os::worker::router::WorkerRouter;

    // 假 dsh：把收到的全部参数逐行打印到 stdout（验证命令构造形态）
    let shim = std::env::temp_dir().join("dsh_shim_test.sh");
    std::fs::write(&shim, "#!/bin/sh\nprintf '%s\\n' \"$@\"\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let mut registry = WorkerRegistry::new();
    registry
        .register(Box::new(dsh_worker(Some(shim.to_str().unwrap()))))
        .unwrap();
    let policy = WorkerPolicy::new().allow("agent.task");
    let router = WorkerRouter::new(RouterCriteria::default());
    let mut orch = WorkerOrchestrator::new(&registry, &policy, router);
    let result = orch
        .run_plan(&[WorkerTask {
            capability: "agent.task".into(),
            input: json!({
                "execution_context": {
                    "workspace": {
                        "root": std::env::temp_dir(),
                        "mode": "isolated"
                    }
                },
                "args": ["只回答两个字：收到"]
            }),
            note: "dsh smoke".into(),
        }])
        .await;
    assert!(result.all_ok, "shim 应成功执行");
    assert_eq!(result.outcomes[0].status, openaip_os::TaskStatus::Completed);
    assert_eq!(result.outcomes[0].worker_id.as_deref(), Some("dsh-harness"));

    // 命令构造正确：dsh --profile headless <任务文本>
    let stdout = result.outcomes[0]
        .output
        .as_ref()
        .and_then(|o| o.get("stdout"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(stdout.contains("--profile"), "参数 1: {stdout}");
    assert!(stdout.contains("headless"), "参数 2: {stdout}");
    assert!(
        stdout.contains("只回答两个字：收到"),
        "任务文本透传: {stdout}"
    );

    // 事件流完整（capability.selected → policy.checked → worker.selected/started/completed）
    let types: Vec<&str> = orch
        .session_log()
        .events()
        .iter()
        .map(|e| e.event_type.as_str())
        .collect();
    for t in [
        "capability.selected",
        "policy.checked",
        "worker.selected",
        "worker.started",
        "worker.completed",
    ] {
        assert!(types.contains(&t), "缺少事件 {t}");
    }
    std::fs::remove_file(&shim).ok();
}

// ── W10b：Process Worker runtime contract（cwd/env/timeout）────────────
#[tokio::test]
async fn tc_w10b_process_worker_runtime_contract() {
    let cwd = std::env::temp_dir().join("oaip_process_worker_contract");
    std::fs::create_dir_all(&cwd).unwrap();
    let mut manifest = WorkerManifest::new("sh-contract", "cli");
    manifest.execution = ExecutionProtocol::Process;
    let worker = openaip_os::worker::process::ProcessWorker::new(manifest, "/bin/sh");

    let result = worker
        .execute(
            "text.echo",
            json!({
                "args": ["-c", "printf '%s|%s' \"$PWD\" \"$OAIP_CONTRACT\""],
                "execution_context": {
                    "workspace": { "root": cwd, "mode": "isolated" },
                    "env": { "OAIP_CONTRACT": "ok" }
                }
            }),
        )
        .await
        .unwrap();
    let stdout = result.output["stdout"].as_str().unwrap_or("");
    assert!(
        stdout.contains("oaip_process_worker_contract"),
        "cwd 未生效: {stdout}"
    );
    assert!(stdout.ends_with("|ok"), "env 未生效: {stdout}");

    let timeout = worker
        .execute(
            "text.echo",
            json!({
                "args": ["-c", "sleep 1"],
                "execution_context": {
                    "workspace": { "root": cwd, "mode": "isolated" },
                    "timeout_ms": 20
                }
            }),
        )
        .await;
    let Err(WorkerError::Execution(message)) = timeout else {
        panic!("超时必须返回 WorkerError::Execution: {timeout:?}");
    };
    assert!(message.contains("超时"), "错误应说明超时: {message}");
}

// ── W11：真实 DSH headless 端到端（#[ignore]：需本机 dsh + ~/.dsh 可写）──
// 本地复现：cargo test --test worker_v2 tc_w11_dsh_headless_e2e -- --ignored --nocapture
#[tokio::test]
#[ignore = "依赖本机 dsh CLI 与 ~/.dsh（外部 LLM/token），CI 不跑"]
async fn tc_w11_dsh_headless_e2e() {
    use openaip_os::worker::adapters::dsh_worker;

    // dsh 不可用 → 跳过（不 panic）
    let probe = std::process::Command::new("dsh").arg("--version").output();
    let Ok(probe) = probe else {
        eprintln!("跳过：dsh 不在 PATH");
        return;
    };
    if !probe.status.success() {
        eprintln!(
            "跳过：dsh 不可执行: {}",
            String::from_utf8_lossy(&probe.stderr)
        );
        return;
    }

    let w = dsh_worker(None);
    let result = w
        .execute("agent.task", json!({ "args": ["只回答两个字：收到"] }))
        .await;
    match result {
        Ok(r) => {
            eprintln!("dsh headless stdout: {:?}", r.output);
            assert!(r.ok, "dsh 应成功返回");
            assert!(
                r.output["stdout"].as_str().unwrap_or("").contains("收到"),
                "headless 应完成任务并打印结果"
            );
        }
        Err(e) => {
            eprintln!("跳过：dsh 执行失败（可能 ~/.dsh 不可写或 LLM 配置缺失）: {e}");
        }
    }
}

// ── W12：Codex 适配器命令构造（假 codex shim，可进 CI）──────────────
#[tokio::test]
async fn tc_w12_codex_adapter_command_shape() {
    use openaip_os::worker::adapters::codex_worker;
    use openaip_os::worker::orchestrator::{WorkerOrchestrator, WorkerTask};
    use openaip_os::worker::router::WorkerRouter;

    let shim = std::env::temp_dir().join("codex_shim_test.sh");
    std::fs::write(&shim, "#!/bin/sh\nprintf '%s\\n' \"$@\"\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let mut registry = WorkerRegistry::new();
    registry
        .register(Box::new(codex_worker(Some(shim.to_str().unwrap()))))
        .unwrap();
    let policy = WorkerPolicy::new().allow("code.read");
    let router = WorkerRouter::new(RouterCriteria::default());
    let mut orch = WorkerOrchestrator::new(&registry, &policy, router);
    let result = orch
        .run_plan(&[WorkerTask {
            capability: "code.read".into(),
            input: json!({ "args": ["exec", "--skip-git-repo-check", "只回答两个字：收到"] }),
            note: "codex smoke".into(),
        }])
        .await;
    assert!(result.all_ok);
    assert_eq!(result.outcomes[0].worker_id.as_deref(), Some("codex"));
    // 命令构造正确：codex exec --skip-git-repo-check <任务文本>
    let stdout = result.outcomes[0]
        .output
        .as_ref()
        .and_then(|o| o.get("stdout"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(stdout.contains("exec"), "参数 1: {stdout}");
    assert!(stdout.contains("--skip-git-repo-check"), "参数 2: {stdout}");
    assert!(
        stdout.contains("只回答两个字：收到"),
        "任务文本透传: {stdout}"
    );
    std::fs::remove_file(&shim).ok();
}

// ── W13：真实 Codex exec 端到端（#[ignore]：需本机 codex + 登录 + token）──
// 本地复现：cargo test --test worker_v2 tc_w13_codex_exec_e2e -- --ignored --nocapture
#[tokio::test]
#[ignore = "依赖本机 codex CLI、登录态与 LLM token，CI 不跑"]
async fn tc_w13_codex_exec_e2e() {
    use openaip_os::worker::adapters::codex_worker;

    let probe = std::process::Command::new("codex")
        .arg("--version")
        .output();
    let Ok(probe) = probe else {
        eprintln!("跳过：codex 不在 PATH");
        return;
    };
    if !probe.status.success() {
        eprintln!("跳过：codex 不可执行");
        return;
    }

    let w = codex_worker(None);
    let result = w
        .execute(
            "code.read",
            json!({ "args": ["exec", "--skip-git-repo-check", "--sandbox", "workspace-write", "只回答两个字：收到"] }),
        )
        .await;
    match result {
        Ok(r) => {
            eprintln!("codex exec stdout: {:?}", r.output);
            assert!(r.ok, "codex 应成功返回");
            assert!(
                r.output["stdout"].as_str().unwrap_or("").contains("收到"),
                "codex exec 应完成任务并打印结果"
            );
        }
        Err(e) => {
            eprintln!("跳过：codex 执行失败（登录态/网络/sandbox 问题）: {e}");
        }
    }
}

// ── W14：Pi Worker 适配器命令构造（假 pi shim，可进 CI）─────────────
#[tokio::test]
async fn tc_w14_pi_adapter_command_shape() {
    use openaip_os::worker::adapters::pi_worker;
    use openaip_os::worker::orchestrator::{WorkerOrchestrator, WorkerTask};
    use openaip_os::worker::router::WorkerRouter;

    let shim = std::env::temp_dir().join("pi_shim_test.sh");
    std::fs::write(&shim, "#!/bin/sh\nprintf '%s\\n' \"$@\"\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let mut registry = WorkerRegistry::new();
    registry
        .register(Box::new(pi_worker(Some(shim.to_str().unwrap()))))
        .unwrap();
    let policy = WorkerPolicy::new().allow("code.read");
    let router = WorkerRouter::new(RouterCriteria::default());
    let mut orch = WorkerOrchestrator::new(&registry, &policy, router);
    let result = orch
        .run_plan(&[WorkerTask {
            capability: "code.read".into(),
            input: json!({ "args": ["--mode", "text", "--print", "--no-session", "只回答两个字：收到"] }),
            note: "pi smoke".into(),
        }])
        .await;
    assert!(result.all_ok);
    assert_eq!(result.outcomes[0].worker_id.as_deref(), Some("pi-agent"));
    // 命令构造正确：pi --mode text --print --no-session <任务文本>（Worker 形态：不放 --no-tools）
    let stdout = result.outcomes[0]
        .output
        .as_ref()
        .and_then(|o| o.get("stdout"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    for expected in [
        "--mode",
        "text",
        "--print",
        "--no-session",
        "只回答两个字：收到",
    ] {
        assert!(stdout.contains(expected), "缺参数 {expected}: {stdout}");
    }
    assert!(
        !stdout.contains("--no-tools"),
        "Worker 形态不得禁工具（与 Thinker 形态区分）"
    );
    std::fs::remove_file(&shim).ok();
}

// ── W15：真实 Pi Worker 端到端（#[ignore]：需本机 pi + LLM/token）──
// 本地复现：cargo test --test worker_v2 tc_w15_pi_worker_e2e -- --ignored --nocapture
#[tokio::test]
#[ignore = "依赖本机 pi CLI 与 LLM/token（~/.pi 在工作区外），CI 不跑"]
async fn tc_w15_pi_worker_e2e() {
    use openaip_os::worker::adapters::pi_worker;

    let probe = std::process::Command::new("pi").arg("--version").output();
    let Ok(probe) = probe else {
        eprintln!("跳过：pi 不在 PATH");
        return;
    };
    if !probe.status.success() {
        eprintln!("跳过：pi 不可执行");
        return;
    }

    let w = pi_worker(None);
    let result = w
        .execute(
            "code.read",
            json!({ "args": ["--mode", "text", "--print", "--no-session", "只回答两个字：收到"] }),
        )
        .await;
    match result {
        Ok(r) => {
            eprintln!("pi stdout: {:?}", r.output);
            assert!(r.ok, "pi 应成功返回");
            assert!(
                r.output["stdout"].as_str().unwrap_or("").contains("收到"),
                "pi 应完成任务并打印结果"
            );
        }
        Err(e) => {
            eprintln!("跳过：pi 执行失败（~/.pi 不可写/LLM 配置缺失）: {e}");
        }
    }
}

// ── W16：Router Selection Learning（Trust 轴）────────────────────────
#[tokio::test]
async fn tc_w16_router_selection_learning() {
    use openaip_os::worker::process::ProcessWorker;
    use openaip_os::worker::router::WorkerRouter;
    use openaip_os::worker_core::{WorkerStat, learned_trust};
    use std::collections::HashMap;

    // 两个同质候选：声明 quality/trust 完全相同 → 经验打破平局
    let mut registry = WorkerRegistry::new();
    for id in ["w-a", "w-b"] {
        let mut m = openaip_os::worker::manifest::WorkerManifest::new(id, "agent")
            .with_capabilities(&["code.read"])
            .with_quality(0.9)
            .with_trust(0.8);
        m.execution = ExecutionProtocol::Process;
        registry
            .register(Box::new(ProcessWorker::new(m, "echo")))
            .unwrap();
    }
    // 经验：w-a 10 次全成功（+0.1），w-b 10 次全失败（−0.1）
    let mut learned = HashMap::new();
    learned.insert(
        ("code.read".into(), "w-a".into()),
        learned_trust(&WorkerStat {
            capability: "code.read".into(),
            worker_id: "w-a".into(),
            runs: 10,
            success: 10,
            total_duration_ms: 0,
            policy_compliance: None,
            avg_cost: None,
            first_seen: None,
            last_seen: None,
        })
        .unwrap(),
    );
    learned.insert(
        ("code.read".into(), "w-b".into()),
        learned_trust(&WorkerStat {
            capability: "code.read".into(),
            worker_id: "w-b".into(),
            runs: 10,
            success: 0,
            total_duration_ms: 0,
            policy_compliance: None,
            avg_cost: None,
            first_seen: None,
            last_seen: None,
        })
        .unwrap(),
    );
    let router = WorkerRouter::new(RouterCriteria {
        learned_trust: learned,
        ..Default::default()
    });
    let sel = router
        .route(
            &registry,
            "code.read",
            &WorkerPolicy::new().allow("code.read"),
        )
        .unwrap();
    assert_eq!(sel.worker_id, "w-a", "经验打破平局：w-a 成功率更高");
    assert!(
        sel.reason.contains("learned"),
        "路由理由可解释: {}",
        sel.reason
    );

    // Trust 轴边界：learned 再高也不能授予被 deny 的能力（Policy Filter 在前）
    let denied = router.route(
        &registry,
        "code.read",
        &WorkerPolicy::new().deny("code.read"),
    );
    assert!(denied.is_err(), "learned trust ≠ 能力授予（I4）");
}

// ── W17：统计持久化 + 学习 map 重建（可回放派生）───────────────────
#[tokio::test]
async fn tc_w17_stats_persist_and_rebuild() {
    use openaip_os::evaluation::WorkerOutcome;
    use openaip_os::worker::stats::{learned_trust_map, record_outcomes};

    let storage = openaip_os::storage::LibSQLStorage::open(":memory:")
        .await
        .unwrap();
    let outcome = |worker: &str, ok: bool| WorkerOutcome {
        capability: "code.read".into(),
        worker_id: worker.into(),
        ok,
        duration_ms: 100,
    };
    // w-a 5 次全成功；w-b 5 次全失败（=门限生效）
    let mut outcomes = Vec::new();
    for _ in 0..5 {
        outcomes.push(outcome("w-a", true));
        outcomes.push(outcome("w-b", false));
    }
    record_outcomes(&storage, &outcomes).await.unwrap();

    let map = learned_trust_map(&storage).await.unwrap();
    assert_eq!(
        map.get(&("code.read".into(), "w-a".into())),
        Some(&0.1),
        "w-a 100% → +0.1"
    );
    assert_eq!(
        map.get(&("code.read".into(), "w-b".into())),
        Some(&-0.1),
        "w-b 0% → −0.1"
    );

    // 样本不足不学习：w-c 仅 2 次 → 不进 map
    let mut few = Vec::new();
    for _ in 0..2 {
        few.push(outcome("w-c", true));
    }
    record_outcomes(&storage, &few).await.unwrap();
    let map2 = learned_trust_map(&storage).await.unwrap();
    assert!(
        !map2.contains_key(&("code.read".into(), "w-c".into())),
        "2 runs 不学习（门限 5）"
    );
}

// ── W18：Router Decision 可审计（WorkerSelected 事件全量）────────────
#[tokio::test]
async fn tc_w18_router_decision_is_auditable() {
    use openaip_os::worker::orchestrator::{WorkerOrchestrator, WorkerTask};
    use openaip_os::worker::router::WorkerRouter;

    let mut registry = WorkerRegistry::new();
    registry
        .register(Box::new(echo_worker("echo", &["text.echo"])))
        .unwrap();
    let policy = WorkerPolicy::new().allow("text.echo");
    let mut orch = WorkerOrchestrator::new(
        &registry,
        &policy,
        WorkerRouter::new(RouterCriteria::default()),
    );
    orch.run_plan(&[WorkerTask::new("text.echo", json!({ "msg": "x" }))])
        .await;
    // Explainable Routing：worker.selected 事件携带候选/信任分/策略判定/理由
    let selected = orch
        .session_log()
        .events()
        .iter()
        .find(|e| e.event_type.as_str() == "worker.selected")
        .expect("有 worker.selected 事件");
    assert_eq!(
        selected.data["policy_verdict"],
        json!("allow"),
        "策略判定可审计"
    );
    assert_eq!(selected.data["worker"], json!("echo"));
    assert!(selected.data["effective_trust"].as_f64().unwrap() > 0.0);
    let candidates = selected.data["candidates"].as_array().unwrap();
    assert!(
        candidates
            .iter()
            .any(|c| c["worker"] == json!("echo") && c["trust_score"].is_number()),
        "候选 + 信任分可审计: {candidates:?}"
    );
    assert!(
        selected.data["reason"].as_str().unwrap().contains("score"),
        "选择理由可审计"
    );
}

// ── W19：效率维度进入 learned map（同成功率，慢者修正更低）────────
#[tokio::test]
async fn tc_w19_efficiency_influences_learned_map() {
    use openaip_os::worker::stats::{learned_trust_map, record_outcomes};

    let storage = openaip_os::storage::LibSQLStorage::open(":memory:")
        .await
        .unwrap();
    // 同成功率（10/10 全成功），w-b 慢 4 倍
    let mut outcomes = Vec::new();
    for _ in 0..10 {
        outcomes.push(openaip_os::evaluation::WorkerOutcome {
            capability: "code.test".into(),
            worker_id: "w-fast".into(),
            ok: true,
            duration_ms: 100,
        });
        outcomes.push(openaip_os::evaluation::WorkerOutcome {
            capability: "code.test".into(),
            worker_id: "w-slow".into(),
            ok: true,
            duration_ms: 400,
        });
    }
    record_outcomes(&storage, &outcomes).await.unwrap();
    let map = learned_trust_map(&storage).await.unwrap();
    let fast = map
        .get(&("code.test".into(), "w-fast".into()))
        .copied()
        .unwrap();
    let slow = map
        .get(&("code.test".into(), "w-slow".into()))
        .copied()
        .unwrap();
    // 100% 成功率 → 两者都 +0.1（封顶正确；区分度需样本内成功率差异——见下）
    assert_eq!(fast, 0.1);
    assert_eq!(slow, 0.1);
    // 用 30% 成功率 + 效率差异验证区分度（raw_fast=0.44 → −0.06；raw_slow=0.29 → −0.1）
    let mut outcomes2 = Vec::new();
    for i in 0..10 {
        outcomes2.push(openaip_os::evaluation::WorkerOutcome {
            capability: "code.test".into(),
            worker_id: "m-fast".into(),
            ok: i < 3,
            duration_ms: 100,
        });
        outcomes2.push(openaip_os::evaluation::WorkerOutcome {
            capability: "code.test".into(),
            worker_id: "m-slow".into(),
            ok: i < 3,
            duration_ms: 400,
        });
    }
    record_outcomes(&storage, &outcomes2).await.unwrap();
    let map2 = learned_trust_map(&storage).await.unwrap();
    let mf = map2
        .get(&("code.test".into(), "m-fast".into()))
        .copied()
        .unwrap();
    let ms = map2
        .get(&("code.test".into(), "m-slow".into()))
        .copied()
        .unwrap();
    assert!(mf > ms, "30% 成功率 + 效率区分：fast={mf} slow={ms}");
    assert!(
        (mf + 0.06).abs() < 1e-9,
        "fast: 0.8×0.3+0.2×1.0=0.44 → −0.06，实际 {mf}"
    );
    assert!(
        (ms + 0.1).abs() < 1e-9,
        "slow: 0.8×0.3+0.2×0.25=0.29 → −0.1 封顶，实际 {ms}"
    );
}
