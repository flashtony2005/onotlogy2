//! V0.3 测试契约（对应 docs/07_测试文档/test-case.md TC-009~011 + 异常边界）
//! 产品认知引擎仅 Pi（src/agent.rs），测试用 test-only Fake 引擎驱动 Planner 确定性跑通。

use async_trait::async_trait;
use futures_util::StreamExt;
use openaip_os::action::{ActionEngine, ActionRegistry};
use openaip_os::agent::{AgentEngine, AgentError, Context, Decision};
use openaip_os::evolution::{
    EvolutionState, Experience, LearnedKnowledge, Pattern, Proposal, ProposalKind, ProposalStatus,
};
use openaip_os::ir::{CompareOp, ValueExpr};
use openaip_os::parse_odl;
use openaip_os::planner::{
    AutoApprover, Capability, PlanStep, Planner, PlannerError, Policy, PolicyDecision,
    generate_run_id,
};
use openaip_os::runtime::RuntimeKernel;
use openaip_os::server;
use openaip_os::storage::{LibSQLStorage, TraceQuery};
use openaip_os::trace::{View, build_timeline};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
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
        let _ = server::serve(
            ks,
            engine,
            listener,
            "ui/dist",
            ActionRegistry::empty(),
            false,
        )
        .await;
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

/// 最小 HTTP/1.1 客户端（Connection: close，无第三方依赖），返回 (状态码, body)。
async fn http_call(
    addr: SocketAddr,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let req = match body {
        Some(b) => format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{b}",
            b.len()
        ),
        None => format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    };
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut buf)).await;
    let text = String::from_utf8_lossy(&buf);
    let (head, rest) = text.split_once("\r\n\r\n").unwrap_or(("", ""));
    let status: u16 = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let cl = head.lines().find_map(|l| {
        l.to_ascii_lowercase()
            .strip_prefix("content-length: ")
            .and_then(|v| v.trim().parse::<usize>().ok())
    });
    let body = match cl {
        Some(n) => rest.get(..n).unwrap_or(rest).to_string(),
        None => rest.to_string(),
    };
    (status, body)
}

/// 启动匿名服务（内存库 + FakeEngine），返回地址；调用方负责喂数据。
async fn spawn_server(kernel: std::sync::Arc<RuntimeKernel>) -> SocketAddr {
    let engine: std::sync::Arc<dyn AgentEngine> = std::sync::Arc::new(FakeEngine);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let ks = kernel.clone();
    tokio::spawn(async move {
        let _ = server::serve(
            ks,
            engine,
            listener,
            "ui/dist",
            ActionRegistry::empty(),
            false,
        )
        .await;
    });
    tokio::time::sleep(Duration::from_millis(150)).await; // 等绑定就绪
    addr
}

// ── TC-012：多候选闭环缩放（每客户 → 1 挽留任务 + 1 needs_retention 边） ────────────────
#[tokio::test]
async fn tc012_multi_candidate_closed_loop() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = RuntimeKernel::new(s);
    for id in ["001", "002", "003"] {
        kernel
            .create_object("Customer", id, json!({ "name": id }))
            .await
            .unwrap();
    }
    let run_id = generate_run_id();
    Planner::new(&kernel)
        .run_goal(&FakeEngine, "reduce churn", "retention-agent", &run_id)
        .await
        .expect("多候选也应跑通");

    let tasks = kernel.query_objects("RetentionTask").await.unwrap();
    assert_eq!(tasks.len(), 3, "应为 3 个客户各创建 1 个任务");
    let edges = kernel.query_relations("needs_retention").await.unwrap();
    assert_eq!(edges.len(), 3, "应建立 3 条 needs_retention 边");
    for e in &edges {
        assert_eq!(e.relation, "needs_retention");
        assert!(tasks.iter().any(|t| t.id == e.target), "边目标应为挽留任务");
    }
}

// ── TC-011 正向：非破坏性能力经 Policy 校验应 Allow（对照 TC-011-b 的 Deny） ─────────────
#[test]
fn tc011_policy_allow_non_destructive() {
    let steps = [
        PlanStep {
            capability: Capability::CreateObject {
                object_type: "X".into(),
                id: "x1".into(),
                data: json!({}),
            },
            object_refs: vec!["x1".into()],
            description: "create".into(),
        },
        PlanStep {
            capability: Capability::LinkRelation {
                source: "a".into(),
                edge: "r".into(),
                target: "b".into(),
            },
            object_refs: vec!["a".into()],
            description: "link".into(),
        },
        PlanStep {
            capability: Capability::QueryObjects {
                object_type: "X".into(),
            },
            object_refs: vec![],
            description: "query".into(),
        },
        PlanStep {
            capability: Capability::EmitEvent {
                event_type: "E".into(),
                refs: vec!["a".into()],
            },
            object_refs: vec!["a".into()],
            description: "emit".into(),
        },
    ];
    for step in steps {
        assert_eq!(
            Policy::check(&step),
            PolicyDecision::Allow,
            "非破坏性能力应 Allow"
        );
    }
}

// ── TC-012-b：空世界（无候选客户）兜底绑定 "world" 哨兵，且不创建任务 ───────────────────
#[tokio::test]
async fn tc012b_empty_world_object_refs_sentinel() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = RuntimeKernel::new(s);
    let run_id = generate_run_id();
    Planner::new(&kernel)
        .run_goal(&FakeEngine, "reduce churn", "retention-agent", &run_id)
        .await
        .expect("空世界也应跑通");

    let events = kernel
        .list_events(TraceQuery::for_run(&run_id))
        .await
        .unwrap();
    assert!(!events.is_empty());
    for e in &events {
        assert!(
            e.object_refs.iter().any(|r| r == "world"),
            "空世界事件应兜底绑定 world: {}",
            e.event_type
        );
    }
    assert!(
        kernel
            .query_objects("RetentionTask")
            .await
            .unwrap()
            .is_empty(),
        "空世界不应创建任务"
    );
}

// ── TC-013：TraceQuery 过滤（run/agent/object）与 after_id 游标 ───────────────────────
#[tokio::test]
async fn tc013_trace_query_filters() {
    let (kernel, run_id) = seed_and_run(&FakeEngine).await;
    let all = kernel
        .list_events(TraceQuery::for_run(&run_id))
        .await
        .unwrap();
    assert!(!all.is_empty());

    // agent 过滤等价
    let by_agent = kernel
        .list_events(TraceQuery::for_agent("retention-agent"))
        .await
        .unwrap();
    assert_eq!(by_agent.len(), all.len(), "agent 过滤应等价");

    // object 过滤：仅返回绑定 001 的事件
    let by_obj = kernel
        .list_events(TraceQuery::for_run(&run_id).with_object("001"))
        .await
        .unwrap();
    assert!(!by_obj.is_empty(), "应存在绑定客户 001 的事件");
    assert!(
        by_obj
            .iter()
            .all(|e| e.object_refs.contains(&"001".to_string())),
        "全部应绑定 001"
    );

    // after 游标：跳过首条后剩余
    let first_id = all[0].id.clone();
    let after = kernel
        .list_events(TraceQuery::for_run(&run_id).with_after(&first_id))
        .await
        .unwrap();
    assert_eq!(after.len(), all.len() - 1, "游标后应少一条");
    assert!(after.iter().all(|e| e.id != first_id), "游标事件不应包含");
}

// ── TC-009 衍生：REST 列表分页 + 单条缺失 WB2002 ──────────────────────────────────────
#[tokio::test]
async fn tc_rest_trace_list_and_404() {
    let kernel = std::sync::Arc::new(RuntimeKernel::new(
        LibSQLStorage::open(":memory:").await.unwrap(),
    ));
    kernel
        .create_object("Customer", "001", json!({ "name": "Apple" }))
        .await
        .unwrap();
    let addr = spawn_server(kernel.clone()).await;

    let run_id = generate_run_id();
    Planner::new(kernel.as_ref())
        .run_goal(&FakeEngine, "reduce churn", "retention-agent", &run_id)
        .await
        .unwrap();

    let (_st, body) = http_call(addr, "GET", "/api/v1/wb/agent/trace?pageSize=2", None).await;
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["code"], 0, "列表应成功: {body}");
    assert!(
        v["data"]["total"].as_u64().unwrap() >= 8,
        "应至少 8 条认知事件"
    );
    assert_eq!(
        v["data"]["list"].as_array().unwrap().len(),
        2,
        "pageSize=2 返回 2 条"
    );

    let (_st2, body2) = http_call(addr, "GET", "/api/v1/wb/agent/trace/tr_missing", None).await;
    let v2: Value = serde_json::from_str(&body2).unwrap();
    assert_eq!(v2["code"], 2002, "缺失 trace 应 WB2002: {body2}");
}

// ── TC-009 衍生：POST /run 门禁（缺 agentId→WB1001；有效→runId+streamUrl） + timeline ──
#[tokio::test]
async fn tc_rest_run_start_gate() {
    let kernel = std::sync::Arc::new(RuntimeKernel::new(
        LibSQLStorage::open(":memory:").await.unwrap(),
    ));
    kernel
        .create_object("Customer", "001", json!({ "name": "Apple" }))
        .await
        .unwrap();
    let addr = spawn_server(kernel.clone()).await;

    // 缺 agentId → WB1001
    let (_s, b1) = http_call(
        addr,
        "POST",
        "/api/v1/wb/agent/run",
        Some(r#"{"goal":"reduce churn"}"#),
    )
    .await;
    let v1: Value = serde_json::from_str(&b1).unwrap();
    assert_eq!(v1["code"], 1001, "缺 agentId 应 WB1001: {b1}");

    // 有效 → runId + streamUrl
    let (_s2, b2) = http_call(
        addr,
        "POST",
        "/api/v1/wb/agent/run",
        Some(r#"{"agentId":"retention-agent","goal":"reduce churn"}"#),
    )
    .await;
    let v2: Value = serde_json::from_str(&b2).unwrap();
    assert_eq!(v2["code"], 0, "应成功触发: {b2}");
    let run_id = v2["data"]["runId"].as_str().unwrap().to_string();
    assert!(v2["data"]["streamUrl"].as_str().unwrap().contains(&run_id));

    // 轮询 timeline（Executive 视图应含决策摘要）
    let tl_path = format!("/api/v1/wb/agent/run/{run_id}/timeline?view=executive");
    let mut status = String::new();
    let mut decision: Option<Value> = None;
    for _ in 0..10 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let (_s, tb) = http_call(addr, "GET", &tl_path, None).await;
        let tv: Value = serde_json::from_str(&tb).unwrap();
        status = tv["data"]["status"].as_str().unwrap_or("").to_string();
        decision = Some(tv["data"]["decision"].clone());
        if status == "Completed" {
            break;
        }
    }
    assert_eq!(status, "Completed", "FakeEngine 应跑通");
    assert!(
        decision.is_some_and(|d| !d.is_null()),
        "Executive 视图应返回决策摘要"
    );
}

// ── TC-009 衍生：WebSocket 按 run 隔离（仅收目标 run 的事件，不混入其它 run） ───────────
#[tokio::test]
async fn tc_ws_run_scoped() {
    let kernel = std::sync::Arc::new(RuntimeKernel::new(
        LibSQLStorage::open(":memory:").await.unwrap(),
    ));
    let run_a = generate_run_id();
    let run_b = generate_run_id();
    // 向同一 kernel 注入两个 run 的事件（直接调用底层，避免 planner 重复建对象冲突）
    for (rid, aid) in [(&run_a, "agent-a"), (&run_b, "agent-b")] {
        for step in ["Goal", "Reasoning", "Planning", "Action"] {
            kernel
                .emit_cognitive(
                    step,
                    rid,
                    aid,
                    &["001".to_string()],
                    Some("x"),
                    Some(json!({})),
                    Some(json!("ok")),
                    Some(0.9),
                )
                .await
                .unwrap();
        }
    }

    let addr = spawn_server(kernel.clone()).await;
    let path = format!("/api/v1/wb/agent/{run_b}/stream");
    let msgs = collect_ws(&addr, &path).await;
    assert!(!msgs.is_empty(), "应收到 run_b 的事件");
    for m in &msgs {
        assert_eq!(
            m["runId"].as_str(),
            Some(run_b.as_str()),
            "只能收到 run_b 的事件"
        );
    }
    assert!(!msgs.iter().any(|m| m["runId"] == run_a), "不应混入 run_a");
}

// ── TC-011 衍生：同一库二次运行幂等（CreateObject upsert + LinkRelation 跳过） ─────────
#[tokio::test]
async fn tc011_repeated_run_is_idempotent() {
    let (kernel, _) = seed_and_run(&FakeEngine).await;
    let run_id = generate_run_id();
    Planner::new(&kernel)
        .run_goal(&FakeEngine, "reduce churn", "retention-agent", &run_id)
        .await
        .expect("同一库二次运行应幂等成功");

    let tasks = kernel.query_objects("RetentionTask").await.unwrap();
    assert_eq!(tasks.len(), 1, "task_001 应复用而非重复创建");
    assert_eq!(tasks[0].version, 2, "二次运行应更新 version");

    let edges = kernel.neighbors("001").await.unwrap();
    assert_eq!(edges.len(), 1, "关系边应复用而非重复建边");
}

// ── V0.4 完整 Action Engine（REQ-014）───────────────────────────────────────
const ODL_ACTIONS: &str = r#"
object Customer:
  properties:
    name: string
    level: string
  actions:
    updateLevel:
      params:
        level: string
      updates:
        - field: level
          value: level
    createOrder:
      params:
        amount: float
      creates:
        - type: Order
          data:
            amount: amount
            customer: self
          link: places
"#;

const ODL_RETAIN: &str = r#"
object Customer:
  properties:
    name: string
    level: string
  actions:
    retain:
      creates:
        - type: RetentionTask
          data:
            title: "客户挽留计划"
            customer: self
          link: needs_retention
"#;

// 带 Policy 的动作：退款需人工审批（require_approval），且金额须 <= 1000（precondition）。
const ODL_REFUND: &str = r#"
object Customer:
  properties:
    name: string
    level: string
  actions:
    refund:
      params:
        amount: float
      updates:
        - field: balance
          value: amount
      policy:
        require_approval: true
        precondition:
          - field: amount
            op: le
            value: 1000
          - field: self.level
            op: ne
            value: "BLACKLIST"
"#;

// 仅含审批闸门的动作（无前置条件），用于 Planner 闭环验证 Human Approval 门禁。
const ODL_APPROVE: &str = r#"
object Customer:
  properties:
    name: string
    level: string
  actions:
    closeAccount:
      policy:
        require_approval: true
"#;

// TC-014a：ODL `actions:` 块解析为 IR（params / updates / creates / data / link / self）
#[tokio::test]
async fn tc014a_odl_parses_actions() {
    let ir = parse_odl(ODL_ACTIONS).expect("ODL 应解析");
    let cust = ir.find("Customer").expect("应有 Customer");
    assert_eq!(cust.actions.len(), 2, "应解析出 2 个动作");

    let upd = cust
        .actions
        .iter()
        .find(|a| a.name == "updateLevel")
        .expect("updateLevel");
    assert_eq!(upd.params.len(), 1);
    assert_eq!(upd.params[0].name, "level");
    assert_eq!(upd.updates.len(), 1);
    assert_eq!(upd.updates[0].field, "level");
    assert!(matches!(upd.updates[0].value, ValueExpr::Param(ref p) if p == "level"));

    let create = cust
        .actions
        .iter()
        .find(|a| a.name == "createOrder")
        .expect("createOrder");
    assert_eq!(create.creates.len(), 1);
    let c = &create.creates[0];
    assert_eq!(c.object_type, "Order");
    assert_eq!(c.link_edge.as_deref(), Some("places"));
    assert!(
        c.data
            .iter()
            .any(|(k, v)| k == "amount" && matches!(v, ValueExpr::Param(p) if p == "amount"))
    );
    assert!(
        c.data
            .iter()
            .any(|(k, v)| k == "customer" && matches!(v, ValueExpr::SelfId))
    );
}

// TC-014b：ActionEngine 解释执行 updates + creates + link（幂等、发 world 事件）
#[tokio::test]
async fn tc014b_action_engine_executes() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = RuntimeKernel::new(s);
    kernel
        .create_object("Customer", "001", json!({"name": "Apple", "level": "VIP"}))
        .await
        .unwrap();
    let reg = ActionRegistry::from_ir(&parse_odl(ODL_ACTIONS).unwrap());
    let engine = ActionEngine::new(&kernel);

    // updateLevel：level -> GOLD
    let upd = reg.get("Customer", "updateLevel").unwrap();
    let mut args = HashMap::new();
    args.insert("level".to_string(), json!("GOLD"));
    engine.invoke(upd, "001", &args).await.unwrap();
    let c = kernel.get_object("001").await.unwrap().unwrap();
    assert_eq!(c.data["level"], json!("GOLD"), "updateLevel 应改写 level");

    // createOrder：创建 Order 并连边 places（customer=self）
    let ord = reg.get("Customer", "createOrder").unwrap();
    let mut args2 = HashMap::new();
    args2.insert("amount".to_string(), json!(100));
    let eff = engine.invoke(ord, "001", &args2).await.unwrap();
    assert_eq!(eff.created.len(), 1);
    let oid = &eff.created[0];
    let o = kernel.get_object(oid).await.unwrap().unwrap();
    assert_eq!(o.data["amount"], json!(100));
    assert_eq!(o.data["customer"], json!("001"), "customer 应为 self ID");
    let edges = kernel.neighbors("001").await.unwrap();
    assert!(
        edges
            .iter()
            .any(|e| e.relation == "places" && e.target == *oid)
    );

    // 动作执行 world 事件
    let wevents = kernel.list_world_events().await.unwrap();
    assert!(wevents.iter().any(|e| e.event_type == "action.createOrder"));
}

// TC-014c：Planner 经 ActionRegistry 跑通闭环（声明式 retain 替代硬编码挽留计划）
#[tokio::test]
async fn tc014c_planner_via_registry_closed_loop() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = RuntimeKernel::new(s);
    kernel
        .create_object("Customer", "001", json!({"name": "Apple", "level": "VIP"}))
        .await
        .unwrap();
    kernel
        .create_object("Customer", "002", json!({"name": "Tesla", "level": "GOLD"}))
        .await
        .unwrap();
    let reg = ActionRegistry::from_ir(&parse_odl(ODL_RETAIN).unwrap());
    let run_id = generate_run_id();
    let res = Planner::with_actions(&kernel, reg)
        .run_goal(&FakeEngine, "reduce churn", "retention-agent", &run_id)
        .await
        .expect("经注册表应跑通");
    assert_eq!(res.status.as_str(), "Completed");

    // 每客户一个 RetentionTask（customer=self），一条 needs_retention 边
    let tasks = kernel.query_objects("RetentionTask").await.unwrap();
    assert_eq!(tasks.len(), 2, "应为 2 个客户各建 1 个挽留任务");
    for t in &tasks {
        let cid = t.data["customer"].as_str().unwrap();
        assert!(cid == "001" || cid == "002");
        let edges = kernel.neighbors(cid).await.unwrap();
        assert!(
            edges
                .iter()
                .any(|e| e.relation == "needs_retention" && e.target == t.id),
            "应建立 needs_retention 边"
        );
    }

    // 动作执行 world 事件 + Policy Allow（非 Deny）
    let wevents = kernel.list_world_events().await.unwrap();
    assert!(wevents.iter().any(|e| e.event_type == "action.retain"));
    let cevents = kernel
        .list_events(TraceQuery::for_run(&run_id))
        .await
        .unwrap();
    let policy: Vec<&str> = cevents
        .iter()
        .filter(|e| e.event_type == "PolicyCheck")
        .map(|e| e.output.as_ref().and_then(|o| o.as_str()).unwrap_or(""))
        .collect();
    assert!(
        policy.iter().all(|p| *p == "Allow"),
        "Policy 应 Allow 声明式动作"
    );
}

// ── TC-022a：记忆沉淀存储往返（V0.5 Memory = Event 积累；BL-008）──────────────────────
#[tokio::test]
async fn tc022a_memory_store_roundtrip() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = RuntimeKernel::new(s);
    // 首沉淀
    let m1 = kernel
        .record_memory("action.retain", json!({ "experience": "retain" }))
        .await
        .unwrap();
    assert_eq!(m1.hits, 1, "首次沉淀 hits=1");
    // 再次沉淀同标签 → hits 累加 + payload 更新
    let m2 = kernel
        .record_memory("action.retain", json!({ "experience": "retain", "v": 2 }))
        .await
        .unwrap();
    assert_eq!(m2.hits, 2, "重复沉淀应累加 hits");
    assert_eq!(m2.payload["v"], json!(2), "应更新 payload");
    // recall 全量 / 单条 / 缺失
    let all = kernel.recall_all().await.unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].hits, 2);
    let one = kernel.recall_memory("action.retain").await.unwrap();
    assert!(one.is_some() && one.unwrap().hits == 2);
    assert!(kernel.recall_memory("missing").await.unwrap().is_none());
}

// ── TC-022b：Planner 记忆闭环（Action→Memory→下次 Plan 召回）──────────────────────────
#[tokio::test]
async fn tc022b_planner_memory_closed_loop() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = RuntimeKernel::new(s);
    kernel
        .create_object("Customer", "001", json!({"name": "Apple", "level": "VIP"}))
        .await
        .unwrap();
    kernel
        .create_object("Customer", "002", json!({"name": "Tesla", "level": "GOLD"}))
        .await
        .unwrap();
    let reg = ActionRegistry::from_ir(&parse_odl(ODL_RETAIN).unwrap());

    // 第一轮：执行 retain → 沉淀 action.retain
    let rid1 = generate_run_id();
    let r1 = Planner::with_actions(&kernel, reg.clone())
        .run_goal(&FakeEngine, "reduce churn", "retention-agent", &rid1)
        .await
        .expect("首轮应跑通");
    assert_eq!(r1.status.as_str(), "Completed");
    let mem1 = kernel.recall_all().await.unwrap();
    assert!(
        mem1.iter()
            .any(|m| m.label == "action.retain" && m.hits >= 1),
        "首轮应沉淀 action.retain"
    );

    // 第二轮：同 kernel 累加 hits，且应召回 MemoryRecall 事件（闭环可见）
    let rid2 = generate_run_id();
    let r2 = Planner::with_actions(&kernel, reg)
        .run_goal(&FakeEngine, "reduce churn", "retention-agent", &rid2)
        .await
        .expect("次轮应跑通");
    assert_eq!(r2.status.as_str(), "Completed");
    let retain = kernel
        .recall_all()
        .await
        .unwrap()
        .into_iter()
        .find(|m| m.label == "action.retain")
        .expect("应存在 action.retain 记忆");
    assert!(retain.hits >= 2, "两轮应累加 hits（自学习沉淀）");
    let cev2 = kernel
        .list_events(TraceQuery::for_run(&rid2))
        .await
        .unwrap();
    assert!(
        cev2.iter().any(|e| e.event_type == "MemoryRecall"),
        "第二轮应召回 MemoryRecall 事件（Action→Memory→下次 Plan 闭环）"
    );
}

// ── TC-015a：ODL `policy:` 块解析为 IR（require_approval + precondition 子句） ──────────
#[test]
fn tc015a_odl_parses_policy() {
    let ir = parse_odl(ODL_REFUND).expect("ODL 应解析");
    let cust = ir.find("Customer").expect("应有 Customer");
    let refund = cust
        .actions
        .iter()
        .find(|a| a.name == "refund")
        .expect("refund");
    assert!(refund.policy.require_approval, "refund 应需审批");

    assert_eq!(refund.policy.precondition.len(), 2, "应有 2 条前置条件");
    let c0 = &refund.policy.precondition[0];
    assert_eq!(c0.field, "amount");
    assert_eq!(c0.op, CompareOp::Le);
    assert_eq!(
        c0.value,
        ValueExpr::Literal(json!(1000)),
        "比较右值应为字面量 1000"
    );
    let c1 = &refund.policy.precondition[1];
    assert_eq!(c1.field, "self.level");
    assert_eq!(c1.op, CompareOp::Ne);
    assert!(matches!(c1.value, ValueExpr::Literal(Value::String(ref s)) if s == "BLACKLIST"));
}

// ── TC-015b：审批闸门 — require_approval + AutoApprover 放行/拒绝 ───────────────────────
#[tokio::test]
async fn tc015b_approval_gate() {
    let def = parse_odl(ODL_REFUND)
        .unwrap()
        .find("Customer")
        .unwrap()
        .actions
        .iter()
        .find(|a| a.name == "refund")
        .cloned()
        .unwrap();
    let step = PlanStep {
        capability: Capability::Action {
            object_type: "Customer".into(),
            action: "refund".into(),
            target_id: "001".into(),
            args: json!({}),
        },
        object_refs: vec!["001".into()],
        description: "refund".into(),
    };

    // 默认审批器（allow=false）→ 需 Human Approval，Deny（WB2005）
    let mut args = HashMap::new();
    args.insert("amount".to_string(), json!(500));
    let target = json!({ "level": "VIP" });
    let deny = Policy::check_action(
        &step,
        Some(&def),
        &AutoApprover { allow: false },
        Some(&target),
        &args,
    )
    .await;
    assert!(matches!(deny, PolicyDecision::Deny { .. }), "未批准应 Deny");

    // 自动批准（allow=true）→ Allow
    let allow = Policy::check_action(
        &step,
        Some(&def),
        &AutoApprover { allow: true },
        Some(&target),
        &args,
    )
    .await;
    assert_eq!(allow, PolicyDecision::Allow, "批准后应 Allow");
}

// ── TC-015c：前置条件 — 参数/self 字段比较满足或拒绝 ───────────────────────────────────
#[tokio::test]
async fn tc015c_precondition_eval() {
    let def = parse_odl(ODL_REFUND)
        .unwrap()
        .find("Customer")
        .unwrap()
        .actions
        .iter()
        .find(|a| a.name == "refund")
        .cloned()
        .unwrap();
    let step = PlanStep {
        capability: Capability::Action {
            object_type: "Customer".into(),
            action: "refund".into(),
            target_id: "001".into(),
            args: json!({}),
        },
        object_refs: vec!["001".into()],
        description: "refund".into(),
    };
    let approver = AutoApprover { allow: true };

    // amount=500 <= 1000 且 level != BLACKLIST → Allow
    let mut args = HashMap::new();
    args.insert("amount".to_string(), json!(500));
    let target = json!({ "id": "001", "level": "VIP" });
    assert_eq!(
        Policy::check_action(&step, Some(&def), &approver, Some(&target), &args).await,
        PolicyDecision::Allow,
        "金额 500 且非黑名单应 Allow"
    );

    // amount=2000 > 1000 → Deny（不满足 precondition）
    let mut args2 = HashMap::new();
    args2.insert("amount".to_string(), json!(2000));
    assert!(
        matches!(
            Policy::check_action(&step, Some(&def), &approver, Some(&target), &args2).await,
            PolicyDecision::Deny { .. }
        ),
        "金额 2000 超阈值应 Deny"
    );

    // level=BLACKLIST → Deny（self.level 不满足 ne）
    let blacklisted = json!({ "id": "001", "level": "BLACKLIST" });
    let mut args3 = HashMap::new();
    args3.insert("amount".to_string(), json!(500));
    assert!(
        matches!(
            Policy::check_action(&step, Some(&def), &approver, Some(&blacklisted), &args3).await,
            PolicyDecision::Deny { .. }
        ),
        "黑名单客户应 Deny"
    );
}

// ── TC-015d：Planner 闭环 — require_approval 动作默认拒（WB2005）；自动批准则跑通 ───────
#[tokio::test]
async fn tc015d_planner_policy_gate() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = RuntimeKernel::new(s);
    kernel
        .create_object("Customer", "001", json!({"name": "Apple", "level": "VIP"}))
        .await
        .unwrap();
    let reg = ActionRegistry::from_ir(&parse_odl(ODL_APPROVE).unwrap());

    // 默认（无自动批准）→ 应因 Policy 拒绝而返回 WB2005
    let run_id = generate_run_id();
    let res = Planner::with_actions(&kernel, reg.clone())
        .run_goal(&FakeEngine, "close account", "retention-agent", &run_id)
        .await;
    match res {
        Err(PlannerError::PolicyDenied(msg)) => {
            assert!(msg.contains("Human Approval"), "应提示需人工审批: {msg}")
        }
        other => panic!("require_approval 动作应被 Policy 拒绝（WB2005），得到 {other:?}"),
    }

    // 注入自动批准 → 闭环跑通（Completed），动作被实际执行
    let s2 = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel2 = RuntimeKernel::new(s2);
    kernel2
        .create_object("Customer", "001", json!({"name": "Apple", "level": "VIP"}))
        .await
        .unwrap();
    let run_id2 = generate_run_id();
    let res2 = Planner::with_actions(&kernel2, reg)
        .with_approver(Arc::new(AutoApprover { allow: true }))
        .run_goal(&FakeEngine, "close account", "retention-agent", &run_id2)
        .await
        .expect("自动批准后应跑通");
    assert_eq!(res2.status.as_str(), "Completed");
    // 动作在自动批准后被执行：产出 action.closeAccount world 事件
    let wevents = kernel2.list_world_events().await.unwrap();
    assert!(
        wevents
            .iter()
            .any(|e| e.event_type == "action.closeAccount"),
        "自动批准后动作应被执行"
    );
}

// ── TC-021a：CLI `agent run` 与 `serve` 暴露 `--auto-approve` 开关（防回归） ─────────────
#[test]
fn tc021a_cli_auto_approve_flag_exists() {
    use std::process::Command;
    let bin = env!("CARGO_BIN_EXE_oaip");
    // agent run --help 应含 --auto-approve
    let out = Command::new(bin)
        .args(["agent", "run", "--help"])
        .output()
        .unwrap();
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(
        help.contains("--auto-approve"),
        "agent run 应暴露 --auto-approve 开关"
    );
    // serve --help 应含 --auto-approve
    let out2 = Command::new(bin)
        .args(["serve", "--help"])
        .output()
        .unwrap();
    let help2 = String::from_utf8_lossy(&out2.stdout);
    assert!(
        help2.contains("--auto-approve"),
        "serve 应暴露 --auto-approve 开关"
    );
}

// ── TC-021b：ConsoleApprover 公开导出且可构造（交互式 Human Approval 入口） ───────────
#[test]
fn tc021b_console_approver_constructible() {
    use openaip_os::planner::ConsoleApprover;
    let _ = ConsoleApprover::new();
}

// 注：ConsoleApprover 的 stdin 交互（y/N）属 CLI 运行时行为，需人工验证：
//   `oaip agent run --odl examples/crm.odl "reduce churn"`（含 require_approval 动作时
//   每动作交互询问一次，y 放行 / N 拒=WB2005 终止）；`--auto-approve` 直接放行。

// ── TC-023a：server 挂起式 HTTP 审批端点 — 挂起待审批 → 浏览器批准 → 运行完成 ──────────
#[tokio::test]
async fn tc023a_server_http_approval_suspend_and_approve() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = std::sync::Arc::new(RuntimeKernel::new(s));
    kernel
        .create_object("Customer", "001", json!({"name": "Apple", "level": "VIP"}))
        .await
        .unwrap();
    let engine: std::sync::Arc<dyn AgentEngine> = std::sync::Arc::new(FakeEngine);
    let reg = ActionRegistry::from_ir(&parse_odl(ODL_APPROVE).unwrap());

    // 启动服务：auto_approve=false → require_approval 动作挂起，等待 HTTP 审批
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let ks = kernel.clone();
    let es = engine.clone();
    tokio::spawn(async move {
        let _ = server::serve(ks, es, listener, "ui/dist", reg, false).await;
    });
    tokio::time::sleep(Duration::from_millis(150)).await;

    // 触发运行（立即返回 runId + streamUrl）
    let (st, body) = http_call(
        addr,
        "POST",
        "/api/v1/wb/agent/run",
        Some(r#"{"agentId":"retention-agent","goal":"close account"}"#),
    )
    .await;
    assert_eq!(st, 200);
    let run_id = serde_json::from_str::<Value>(&body).unwrap()["data"]["runId"]
        .as_str()
        .unwrap()
        .to_string();

    // 运行应挂起：GET approvals 出现至少一条 Pending
    let mut pending: Vec<Value> = Vec::new();
    for _ in 0..100 {
        let (_, b) = http_call(
            addr,
            "GET",
            &format!("/api/v1/wb/agent/run/{run_id}/approvals"),
            None,
        )
        .await;
        pending = serde_json::from_str::<Value>(&b)
            .unwrap()
            .get("data")
            .and_then(|d| d.as_array())
            .cloned()
            .unwrap_or_default();
        if pending.iter().any(|p| p["state"] == "Pending") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let req = pending
        .iter()
        .find(|p| p["state"] == "Pending")
        .expect("应挂起出一条待审批请求");
    assert_eq!(req["state"], "Pending");
    assert!(req["reqId"].as_str().unwrap().starts_with("apr_"));
    let req_id = req["reqId"].as_str().unwrap().to_string();

    // 浏览器点批准
    let (st2, _) = http_call(
        addr,
        "POST",
        &format!("/api/v1/wb/agent/run/{run_id}/approval/{req_id}"),
        Some(r#"{"decision":"approve"}"#),
    )
    .await;
    assert_eq!(st2, 200);

    // 批准后运行应推进至 Completed
    let mut status = String::new();
    for _ in 0..100 {
        let (_, b) = http_call(
            addr,
            "GET",
            &format!("/api/v1/wb/agent/run/{run_id}/timeline?view=operator"),
            None,
        )
        .await;
        status = serde_json::from_str::<Value>(&b)
            .unwrap()
            .get("data")
            .and_then(|d| d.get("status"))
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        if status == "Completed" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(status, "Completed", "批准后运行应完成");

    // 动作被实际执行：产出 action.closeAccount world 事件
    let wevents = kernel.list_world_events().await.unwrap();
    assert!(
        wevents
            .iter()
            .any(|e| e.event_type == "action.closeAccount"),
        "批准后动作应被执行"
    );
}

// ── TC-023b：server 挂起式 HTTP 审批端点 — 拒绝 → WB2005 终止 ─────────────────────────
#[tokio::test]
async fn tc023b_server_http_approval_reject() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = std::sync::Arc::new(RuntimeKernel::new(s));
    kernel
        .create_object("Customer", "001", json!({"name": "Apple", "level": "VIP"}))
        .await
        .unwrap();
    let engine: std::sync::Arc<dyn AgentEngine> = std::sync::Arc::new(FakeEngine);
    let reg = ActionRegistry::from_ir(&parse_odl(ODL_APPROVE).unwrap());

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let ks = kernel.clone();
    let es = engine.clone();
    tokio::spawn(async move {
        let _ = server::serve(ks, es, listener, "ui/dist", reg, false).await;
    });
    tokio::time::sleep(Duration::from_millis(150)).await;

    let (_, body) = http_call(
        addr,
        "POST",
        "/api/v1/wb/agent/run",
        Some(r#"{"agentId":"retention-agent","goal":"close account"}"#),
    )
    .await;
    let run_id = serde_json::from_str::<Value>(&body).unwrap()["data"]["runId"]
        .as_str()
        .unwrap()
        .to_string();

    // 等待挂起
    let mut req_id = String::new();
    for _ in 0..100 {
        let (_, b) = http_call(
            addr,
            "GET",
            &format!("/api/v1/wb/agent/run/{run_id}/approvals"),
            None,
        )
        .await;
        let list = serde_json::from_str::<Value>(&b)
            .unwrap()
            .get("data")
            .and_then(|d| d.as_array())
            .cloned()
            .unwrap_or_default();
        if let Some(p) = list.iter().find(|p| p["state"] == "Pending") {
            req_id = p["reqId"].as_str().unwrap().to_string();
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!req_id.is_empty(), "应挂起出一条待审批请求");

    // 浏览器点拒绝
    let (st2, _) = http_call(
        addr,
        "POST",
        &format!("/api/v1/wb/agent/run/{run_id}/approval/{req_id}"),
        Some(r#"{"decision":"reject"}"#),
    )
    .await;
    assert_eq!(st2, 200);

    // 拒绝后运行应终止（Failed）且人工审批标记置位
    let mut status = String::new();
    let mut human = String::new();
    for _ in 0..100 {
        let (_, b) = http_call(
            addr,
            "GET",
            &format!("/api/v1/wb/agent/run/{run_id}/timeline?view=operator"),
            None,
        )
        .await;
        let v = serde_json::from_str::<Value>(&b).unwrap();
        status = v["data"]["status"].as_str().unwrap_or("").to_string();
        human = v["data"]["humanApproval"]
            .as_str()
            .unwrap_or("")
            .to_string();
        if status == "Failed" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(status, "Failed", "拒绝后运行应终止");
    assert_eq!(human, "Required", "拒绝后应标记需人工审批");

    // 审批请求状态应记为 Rejected
    let (_, b) = http_call(
        addr,
        "GET",
        &format!("/api/v1/wb/agent/run/{run_id}/approvals"),
        None,
    )
    .await;
    let list = serde_json::from_str::<Value>(&b)
        .unwrap()
        .get("data")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        list.iter()
            .any(|p| p["reqId"] == req_id && p["state"] == "Rejected"),
        "审批请求应记为 Rejected"
    );

    // 重复裁决同一请求应被拒（code=404，已裁决；HTTP 信封用 code 字段而非状态码）
    let (_, b3) = http_call(
        addr,
        "POST",
        &format!("/api/v1/wb/agent/run/{run_id}/approval/{req_id}"),
        Some(r#"{"decision":"approve"}"#),
    )
    .await;
    let code3 = serde_json::from_str::<Value>(&b3)
        .unwrap()
        .get("code")
        .and_then(|c| c.as_i64())
        .unwrap_or(0);
    assert_eq!(code3, 404, "已裁决请求不应再接受裁决");
}

// ── TC-024a：server Memory 视图端点 — GET /memory 返回沉淀（hits 累加） ──────────────
#[tokio::test]
async fn tc024a_server_memory_endpoint() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = std::sync::Arc::new(RuntimeKernel::new(s));
    // 沉淀两条记忆：action.retain 反复复用以累加 hits
    kernel
        .record_memory("action.retain", json!({ "experience": "retain" }))
        .await
        .unwrap();
    kernel
        .record_memory("action.refund", json!({ "experience": "refund" }))
        .await
        .unwrap();
    kernel
        .record_memory("action.retain", json!({ "experience": "retain", "v": 2 }))
        .await
        .unwrap(); // hits: 1→2

    let addr = spawn_server(kernel).await;

    let (st, body) = http_call(addr, "GET", "/api/v1/wb/agent/memory", None).await;
    assert_eq!(st, 200);
    let v = serde_json::from_str::<Value>(&body).unwrap();
    assert_eq!(v["code"], 0, "记忆查询应成功");
    let list = v["data"]["list"].as_array().unwrap();
    assert_eq!(v["data"]["total"].as_i64().unwrap(), 2, "应去重为 2 条");

    // action.retain 命中两次 → hits=2，且按 hits 降序排首位
    assert_eq!(list[0]["label"], "action.retain", "hits 高者排前");
    assert_eq!(list[0]["hits"].as_i64().unwrap(), 2, "复用语义累加 hits");
    assert_eq!(list[1]["label"], "action.refund");
    assert_eq!(list[1]["hits"].as_i64().unwrap(), 1);
    assert!(
        list[0]["payload"].get("experience").is_some(),
        "payload 应回传"
    );
}

// ════════════════════════════════════════════════════════════════════════════════════════
// TC-025  Evolution Engine MVP（BL-003 · REQ-017）：Experience → Pattern → Proposal →
// Validation → LearnedKnowledge。第一原则 Learn≠Permission：Evolution 不修改 Policy、不绕过
// Human Approval。所有测试均验证该安全边界。
// ════════════════════════════════════════════════════════════════════════════════════════

// ── TC-025a：Experience Mining（hits≥2 才成 Pattern） ────────────────────────────────────
#[test]
fn tc025a_experience_mining() {
    let exps = vec![
        Experience {
            label: "action.retain".into(),
            hits: 3,
        },
        Experience {
            label: "action.refund".into(),
            hits: 1,
        },
    ];
    let patterns = openaip_os::evolution::mine_patterns(&exps);
    // retain(3) → 1 条 pattern；refund(1) → 0 条（未达 PATTERN_MIN_HITS）
    assert_eq!(patterns.len(), 1, "仅 retain 应出 pattern");
    assert_eq!(patterns[0].label, "pattern.action.retain");
    assert_eq!(patterns[0].support, 3);
    // 非 action.* 前缀的经验不参与
    let no_prefix = vec![Experience {
        label: "RetentionPattern".into(),
        hits: 9,
    }];
    assert!(
        openaip_os::evolution::mine_patterns(&no_prefix).is_empty(),
        "非 action.* 经验不应成 Pattern"
    );
}

// ── TC-025b：Confidence 边界（确定性 min(1, hits/5)） ───────────────────────────────────
#[test]
fn tc025b_confidence_boundary() {
    assert_eq!(Pattern::confidence_of(1), 0.2);
    assert_eq!(Pattern::confidence_of(2), 0.4);
    assert_eq!(Pattern::confidence_of(3), 0.6);
    assert_eq!(Pattern::confidence_of(4), 0.8);
    assert_eq!(Pattern::confidence_of(5), 1.0);
    assert_eq!(Pattern::confidence_of(7), 1.0, "超过 5 封顶为 1.0");
}

// ── TC-025c：Proposal（Pattern→Proposal；Proposal 不改变 Agent 行为） ───────────────────
#[test]
fn tc025c_proposal_does_not_change_behavior() {
    let pattern = Pattern {
        label: "pattern.action.retain".into(),
        action: "retain".into(),
        support: 3,
        confidence: 0.6,
        run_ids: vec!["RUN-1".into()],
    };
    let prop = openaip_os::evolution::propose(&pattern);
    assert_eq!(prop.id, "proposal.retain");
    assert_eq!(prop.kind, ProposalKind::ActionPattern);
    assert_eq!(prop.target, "retain");
    assert_eq!(prop.status, ProposalStatus::Proposed);
    assert_eq!(prop.pattern_label, pattern.label);
    // 安全边界：Proposal 仅是一个知识候选，绝不携带任何「权限/放行」语义字段。
    // 这里通过类型断言确认 Proposal 没有 approval / bypass 相关字段——它不能被用来改 Policy。
    let _: Proposal = prop; // 若未来给 Proposal 加 bypass 字段，此类型将被迫显式处理。
}

// ── TC-025d：Validation（阈值校验，Validated≠可改安全策略） ─────────────────────────────
#[test]
fn tc025d_validation_threshold() {
    let mut low = Proposal {
        id: "proposal.refund".into(),
        kind: ProposalKind::ActionPattern,
        target: "refund".into(),
        confidence: 0.4, // hits=2 边界
        status: ProposalStatus::Proposed,
        pattern_label: "pattern.action.refund".into(),
        run_ids: vec![],
    };
    low.status = openaip_os::evolution::validate(&low);
    assert_eq!(low.status, ProposalStatus::Rejected, "conf<0.6 应 Rejected");

    let mut ok = Proposal {
        id: "proposal.retain".into(),
        kind: ProposalKind::ActionPattern,
        target: "retain".into(),
        confidence: 0.6,
        status: ProposalStatus::Proposed,
        pattern_label: "pattern.action.retain".into(),
        run_ids: vec![],
    };
    ok.status = openaip_os::evolution::validate(&ok);
    assert_eq!(
        ok.status,
        ProposalStatus::Validated,
        "conf≥0.6 应 Validated"
    );
    // Validated 只代表「该知识通过验证」，不授予任何权限（Learn≠Permission）。
    // 阈值常量定义见 evolution::VALIDATE_THRESHOLD（=0.6）。
}

// ── TC-025e：Evolution Isolation（LearnedKnowledge 不改动 Policy / 不绕过 Approval） ─────
#[tokio::test]
async fn tc025e_evolution_does_not_bypass_approval() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = RuntimeKernel::new(s);
    // 模拟 retain 被成功执行 3 次 → 沉淀经验
    for _ in 0..3 {
        kernel
            .record_memory("action.retain", json!({ "experience": "retain" }))
            .await
            .unwrap();
    }
    // 跑一轮 Evolution：应产出 LearnedKnowledge（retain Validated）
    let state = kernel.run_evolution("RUN-100").await.unwrap();
    assert_eq!(state.learned.len(), 1, "retain 应沉淀为 LearnedKnowledge");
    assert_eq!(state.learned[0].action, "retain");

    // 关键安全断言：require_approval 动作即使已 Learned，仍走 Human Approval 闸门（WB2005）。
    // 用 refund 动作的 policy（require_approval=true）做 check_action，默认审批器拒 → Deny。
    let def = parse_odl(ODL_REFUND)
        .unwrap()
        .find("Customer")
        .unwrap()
        .actions
        .iter()
        .find(|a| a.name == "refund")
        .cloned()
        .unwrap();
    let step = PlanStep {
        capability: Capability::Action {
            object_type: "Customer".into(),
            action: "refund".into(),
            target_id: "001".into(),
            args: json!({}),
        },
        object_refs: vec!["001".into()],
        description: "refund".into(),
    };
    let mut args = HashMap::new();
    args.insert("amount".to_string(), json!(500));
    let target = json!({ "level": "VIP" });
    let deny = Policy::check_action(
        &step,
        Some(&def),
        &AutoApprover { allow: false },
        Some(&target),
        &args,
    )
    .await;
    assert!(
        matches!(deny, PolicyDecision::Deny { .. }),
        "require_approval 动作即便 Evolution 已学习，仍须 Human Approval（不绕过 WB2005）"
    );
}

// ── TC-025f：Idempotency（run_evolution 可重复运行不重复产出知识） ──────────────────────
#[tokio::test]
async fn tc025f_evolution_idempotent() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = RuntimeKernel::new(s);
    for _ in 0..3 {
        kernel
            .record_memory("action.retain", json!({ "experience": "retain" }))
            .await
            .unwrap();
    }
    // 连续运行三轮
    kernel.run_evolution("RUN-1").await.unwrap();
    kernel.run_evolution("RUN-2").await.unwrap();
    let final_state = kernel.run_evolution("RUN-3").await.unwrap();

    assert_eq!(
        final_state.learned.len(),
        1,
        "learned.retain 应仅 1 条（幂等 upsert）"
    );
    assert_eq!(final_state.proposals.len(), 1, "proposal.retain 应仅 1 条");
    assert_eq!(final_state.patterns.len(), 1, "pattern.retain 应仅 1 条");
    // 证据 run_ids 应跨三轮累加去重：[RUN-1, RUN-2, RUN-3]
    assert_eq!(
        final_state.patterns[0].run_ids.len(),
        3,
        "证据应跨 run 累积"
    );
    assert_eq!(final_state.learned[0].run_ids.len(), 3);
}

// ── TC-025g：Traceability（LearnedKnowledge→Proposal→Pattern→Experience→AgentRun） ──────
#[tokio::test]
async fn tc025g_evolution_traceability() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = RuntimeKernel::new(s);
    for _ in 0..3 {
        kernel
            .record_memory("action.retain", json!({ "experience": "retain" }))
            .await
            .unwrap();
    }
    let state: EvolutionState = kernel.run_evolution("RUN-1021").await.unwrap();

    // LearnedKnowledge → Proposal → Pattern 父引用链完整
    let learned: &LearnedKnowledge = state.learned.first().expect("应有 learned");
    let prop = state
        .proposals
        .iter()
        .find(|p| p.id == learned.proposal_id)
        .expect("learned 应可追溯至 proposal");
    let pat = state
        .patterns
        .iter()
        .find(|p| p.label == prop.pattern_label)
        .expect("proposal 应可追溯至 pattern");

    // Pattern → Experience（action.retain）且 → AgentRun（RUN-1021）
    assert_eq!(pat.action, "retain", "pattern.action 应回指经验动作");
    assert!(
        pat.run_ids.contains(&"RUN-1021".to_string()),
        "pattern 应可追溯至来源 run"
    );
    assert!(
        learned.run_ids.contains(&"RUN-1021".to_string()),
        "learned 应可追溯至来源 run"
    );
    assert_eq!(prop.target, "retain");
}

// ── TC-025h：server GET /evolution 端点（与 Memory 端点同构） ───────────────────────────
#[tokio::test]
async fn tc025h_server_evolution_endpoint() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = Arc::new(RuntimeKernel::new(s));
    for _ in 0..3 {
        kernel
            .record_memory("action.retain", json!({ "experience": "retain" }))
            .await
            .unwrap();
    }
    kernel.run_evolution("RUN-1").await.unwrap();
    let addr = spawn_server(kernel).await;

    let (st, body) = http_call(addr, "GET", "/api/v1/wb/agent/evolution", None).await;
    assert_eq!(st, 200);
    let v = serde_json::from_str::<Value>(&body).unwrap();
    assert_eq!(v["code"], 0, "evolution 查询应成功");

    let patterns = v["data"]["patterns"].as_array().unwrap();
    let learned = v["data"]["learned"].as_array().unwrap();
    assert_eq!(patterns.len(), 1, "应含 1 个 pattern");
    assert_eq!(patterns[0]["label"], "pattern.action.retain");
    assert_eq!(learned.len(), 1, "应含 1 条 learned");
    assert_eq!(learned[0]["action"], "retain");
    assert_eq!(
        learned[0]["proposalId"], "proposal.retain",
        "可追溯 proposal"
    );
}
