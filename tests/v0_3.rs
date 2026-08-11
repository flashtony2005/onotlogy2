//! V0.3 测试契约（对应 docs/07_测试文档/test-case.md TC-009~011 + 异常边界）
//! 产品认知引擎仅 Pi（src/agent.rs），测试用 test-only Fake 引擎驱动 Planner 确定性跑通。

use async_trait::async_trait;
use futures_util::StreamExt;
use openaip_os::agent::{AgentEngine, AgentError, Context, Decision};
use openaip_os::planner::{
    Capability, PlanStep, Planner, PlannerError, Policy, PolicyDecision, generate_run_id,
};
use openaip_os::runtime::RuntimeKernel;
use openaip_os::server;
use openaip_os::storage::{LibSQLStorage, TraceQuery};
use openaip_os::trace::{View, build_timeline};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

/// 确定性 Fake 认知引擎（仅测试，不发货）：按 step 返回结构化决策，绑定世界对象。
struct FakeEngine;

#[async_trait]
impl AgentEngine for FakeEngine {
    fn name(&self) -> &str {
        "fake"
    }
    async fn reason(&self, ctx: &Context) -> Result<Decision, AgentError> {
        let intent = match ctx.step.as_str() {
            "Goal" => "Customer Retention",
            "Intent" => "Reduce churn for at-risk customers",
            "Reasoning" => "Payment delay and order drop signal churn risk",
            "Planning" => "Create retention tasks and link relations",
            _ => ctx.step.as_str(),
        }
        .to_string();
        let refs: Vec<String> = ctx
            .world
            .objects
            .iter()
            .filter_map(|o| o.get("id").and_then(|x| x.as_str()).map(|s| s.to_string()))
            .collect();
        Ok(Decision {
            intent,
            reasoning: format!("fake reasoning for {}", ctx.step),
            confidence: 0.9,
            object_refs: refs,
            output: json!({ "step": ctx.step }),
        })
    }
}

/// 不可用引擎：reason 直接返回 Unavailable（映射 WB2004，TC-010-b）。
struct UnavailableEngine;

#[async_trait]
impl AgentEngine for UnavailableEngine {
    fn name(&self) -> &str {
        "unavailable"
    }
    async fn reason(&self, _ctx: &Context) -> Result<Decision, AgentError> {
        Err(AgentError::Unavailable(
            "test: cognitive engine down".into(),
        ))
    }
    async fn health(&self) -> bool {
        false
    }
}

/// 种子一个客户并跑通 Planner 闭环，返回 (kernel, run_id)。
async fn seed_and_run(engine: &dyn AgentEngine) -> (RuntimeKernel, String) {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = RuntimeKernel::new(s);
    kernel
        .create_object("Customer", "001", json!({"name": "Apple", "level": "VIP"}))
        .await
        .unwrap();
    let run_id = generate_run_id();
    Planner::new(&kernel)
        .run_goal(engine, "reduce churn", "retention-agent", &run_id)
        .await
        .expect("planner 应跑通");
    (kernel, run_id)
}

// ── TC-010：Pi（AgentEngine）reason() 每步产出 cognitive 事件且绑定 object_refs ──────────
#[tokio::test]
async fn tc010_cognitive_trace_with_refs() {
    let (kernel, run_id) = seed_and_run(&FakeEngine).await;
    let events = kernel
        .list_events(TraceQuery::for_run(&run_id))
        .await
        .unwrap();

    // 全部为 cognitive 事件
    assert!(!events.is_empty());
    for e in &events {
        assert_eq!(e.kind.as_str(), "cognitive", "必须是 cognitive 事件");
        assert!(
            !e.object_refs.is_empty(),
            "每步必须绑定 object_refs（可解释性）"
        );
    }

    // 8 个认知步类型齐备
    let types: Vec<&str> = events.iter().map(|e| e.event_type.as_str()).collect();
    for step in [
        "Goal",
        "Intent",
        "OntologyQuery",
        "Reasoning",
        "Planning",
        "PolicyCheck",
        "Action",
        "MemoryUpdate",
    ] {
        assert!(types.contains(&step), "缺少认知步 {step}");
    }

    // Goal 只发射一次（LLMGoalParser 不再重复发事件）
    let goal_count = events.iter().filter(|e| e.event_type == "Goal").count();
    assert_eq!(goal_count, 1, "Goal 应只发一次，实际 {goal_count} 次");
}

// ── TC-011：Planner 全链路 + 最小闭环（创建对象）+ PlanStep 绑定 object_refs ─────────────
#[tokio::test]
async fn tc011_planner_closed_loop() {
    let (kernel, run_id) = seed_and_run(&FakeEngine).await;

    // 规划步经 Trace 回放且绑定对象：Planning 事件存在，Action 事件绑定对象
    let events = kernel
        .list_events(TraceQuery::for_run(&run_id))
        .await
        .unwrap();
    let planning = events.iter().find(|e| e.event_type == "Planning").unwrap();
    assert!(
        planning.object_refs.contains(&"001".to_string()),
        "Planning 应绑定客户 001"
    );
    let action = events.iter().find(|e| e.event_type == "Action").unwrap();
    assert!(!action.object_refs.is_empty(), "Action 必须绑定对象");

    // 最小闭环：RetentionTask 对象被实际创建（Agent 行动作用于世界）
    let tasks = kernel.query_objects("RetentionTask").await.unwrap();
    assert_eq!(tasks.len(), 1, "应为客户 001 创建 1 个挽留任务");
    assert_eq!(
        tasks[0].data.get("customer").and_then(|v| v.as_str()),
        Some("001")
    );

    // PolicyCheck 为 Allow（闭环未被拒）
    let policy = events
        .iter()
        .find(|e| e.event_type == "PolicyCheck")
        .unwrap();
    assert_eq!(policy.output, Some(Value::String("Allow".into())));
}

// ── TC-010-b：Pi 进程未就绪 → WB2004（运行返回 EngineUnavailable） ───────────────────
#[tokio::test]
async fn tc010b_engine_unavailable_wb2004() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = RuntimeKernel::new(s);
    let run_id = generate_run_id();
    let err = Planner::new(&kernel)
        .run_goal(
            &UnavailableEngine,
            "reduce churn",
            "retention-agent",
            &run_id,
        )
        .await
        .expect_err("引擎不可用应返回错误");
    assert!(
        matches!(err, PlannerError::EngineUnavailable(_)),
        "应为 WB2004 EngineUnavailable"
    );
}

// ── TC-011-b：Policy 拒绝某 Action → WB2005，等待 Human Approval ──────────────────────
#[tokio::test]
async fn tc011b_policy_deny_wb2005() {
    let delete_step = PlanStep {
        capability: Capability::DeleteObject { id: "001".into() },
        object_refs: vec!["001".into()],
        description: "删除客户 001".into(),
    };
    let decision = Policy::check(&delete_step);
    assert!(
        matches!(decision, PolicyDecision::Deny { .. }),
        "破坏性动作应被 Policy 拒绝（WB2005）"
    );
}

// ── TC-009：三层视图过滤（Executive 隐藏思考 / Developer 含 IO / Operator 全 Pipeline） ──
#[tokio::test]
async fn tc009_three_view_filtering() {
    let (kernel, run_id) = seed_and_run(&FakeEngine).await;
    let events = kernel
        .list_events(TraceQuery::for_run(&run_id))
        .await
        .unwrap();

    let exec = build_timeline(
        events.clone(),
        run_id.clone(),
        "retention-agent".into(),
        View::Executive,
    );
    let ops = build_timeline(
        events.clone(),
        run_id.clone(),
        "retention-agent".into(),
        View::Operator,
    );
    let dev = build_timeline(
        events.clone(),
        run_id.clone(),
        "retention-agent".into(),
        View::Developer,
    );

    // Executive：仅 Mission/Decision 摘要，隐藏 reasoning
    assert!(
        exec.events.iter().all(|e| e.reasoning.is_none()),
        "Executive 应隐藏思考细节"
    );
    assert!(exec.events.iter().any(|e| e.event_type == "Goal"));
    assert!(exec.decision.is_some(), "Executive 应返回决策摘要");
    assert_eq!(exec.status, "Completed");

    // Operator：完整 Pipeline 步骤（8 步）
    assert_eq!(ops.events.len(), 8, "Operator 应展示全 Pipeline");
    assert!(
        ops.events.iter().all(|e| e.reasoning.is_none()),
        "Operator 不展示推理细节"
    );

    // Developer：含完整 IO（reasoning 可见）
    assert!(
        dev.events.iter().any(|e| e.reasoning.is_some()),
        "Developer 应暴露推理"
    );
    assert!(
        dev.events.iter().any(|e| e.output.is_some()),
        "Developer 应暴露 output"
    );
}

// ── TC-009：WebSocket 实时流 + 断线 `?after` 重连补齐 ────────────────────────────────
#[tokio::test]
async fn tc009_ws_stream_and_reconnect() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = std::sync::Arc::new(RuntimeKernel::new(s));
    kernel
        .create_object("Customer", "001", json!({"name": "Apple", "level": "VIP"}))
        .await
        .unwrap();
    let run_id = generate_run_id();
    let engine: std::sync::Arc<dyn AgentEngine> = std::sync::Arc::new(FakeEngine);

    // 启动服务（同一 kernel，EventBus 共享）
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let ks = kernel.clone();
    tokio::spawn(async move {
        let _ = server::serve(ks, engine, listener, "ui/dist").await;
    });

    // 先跑完 Planner（事件已落库，WS 连接时由 history 补齐）
    Planner::new(kernel.as_ref())
        .run_goal(&FakeEngine, "reduce churn", "retention-agent", &run_id)
        .await
        .unwrap();

    // 首次连接：收全量认知事件
    let path = format!("/api/v1/wb/agent/{run_id}/stream");
    let first = collect_ws(&addr, &path).await;
    assert!(!first.is_empty(), "WS 应推送事件");
    assert!(
        first.iter().any(|m| m["eventType"] == "Goal"),
        "WS 应推送 Goal 事件"
    );
    assert!(
        first.iter().all(|m| m["objectRefs"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false)),
        "WS 推送的事件必须携带 objectRefs"
    );

    // 断线重连：带 ?after=首条 id，应补齐剩余事件
    let first_id = first[0]["id"].as_str().unwrap().to_string();
    let after_path = format!("/api/v1/wb/agent/{run_id}/stream?after={first_id}");
    let rest = collect_ws(&addr, &after_path).await;
    assert_eq!(
        rest.len(),
        first.len() - 1,
        "重连 after 首条后，应补齐剩余 {} 条",
        first.len() - 1
    );
    assert!(
        rest.iter().all(|m| m["id"] != first_id),
        "补齐事件不应包含 after 游标本身"
    );
}

/// 连接 WS 并收集消息（2s 超时后断开）。
async fn collect_ws(addr: &SocketAddr, path: &str) -> Vec<Value> {
    let url = format!("ws://{addr}{path}");
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    let mut out = Vec::new();
    let sleep = tokio::time::sleep(Duration::from_secs(2));
    tokio::pin!(sleep);
    loop {
        tokio::select! {
            msg = ws.next() => {
                match msg {
                    Some(Ok(Message::Text(t))) => {
                        out.push(serde_json::from_str(&t).unwrap());
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
            _ = &mut sleep => break,
        }
    }
    let _ = ws.close(None).await;
    out
}
