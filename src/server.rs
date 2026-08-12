//! Trace API 服务（对应 REQ-009 / api-agent-trace.md）
//!
//! - REST：`GET /trace`、`GET /trace/{id}`、`GET /run/{runId}/timeline?view=`、`POST /run`
//! - Human Approval（V0.4 遗留）：`GET /run/{runId}/approvals`（待审批列表）、
//!   `POST /run/{runId}/approval/{reqId}`（裁决 `{"decision":"approve"|"reject"}`，
//!   `require_approval` 动作挂起等待浏览器点批准/拒绝后恢复或 WB2005 终止）
//! - WebSocket：`/api/v1/wb/agent/{id}/stream`（历史 + 增量 + `?after` 断线重连补齐）
//! - 静态托管：三层观察 UI 构建产物 `ui/dist`（不存在时返回提示）
//!
//! 所有事件同源 `TraceEvent`，WS 与 timeline 共用 `TraceEventDto`（camelCase 单一真相源）。

use crate::action::ActionRegistry;
use crate::agent::AgentEngine;
use crate::evolution::{
    EvolutionState, LearnedKnowledge, Pattern, Proposal, ProposalKind, ProposalStatus,
};
use crate::planner::{
    ApprovalRequest, ApprovalStore, AutoApprover, Planner, PlannerError, PolicyApprover, RunStatus,
    ServerApprover, generate_run_id,
};
use crate::runtime::RuntimeKernel;
use crate::storage::TraceQuery;
use crate::trace::{RunTimeline, TraceEventDto, View, build_timeline};
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{OriginalUri, Path, Query, State, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

/// 运行记录（内存态，供 timeline 状态覆盖与 WB2004/2005 快速反馈）。
struct RunRecord {
    status: RunStatus,
    /// 仅 Policy 拒绝才要求 Human Approval（WB2005）。
    human_approval: bool,
}

/// 共享应用状态。
pub struct AppState {
    pub kernel: Arc<RuntimeKernel>,
    pub engine: Arc<dyn AgentEngine>,
    /// 动作注册表（V0.4）：对象自有动作的声明式来源；空 → legacy 兜底。
    pub registry: ActionRegistry,
    /// 是否自动批准需审批动作（Human Approval）：默认 false → require_approval 动作挂起等待 HTTP 审批。
    auto_approve: bool,
    /// 待审批请求存储（V0.4 遗留的 server 挂起式 HTTP 审批端点）；连接 Planner 挂起点与审批 API。
    store: Arc<ApprovalStore>,
    runs: Mutex<HashMap<String, RunRecord>>,
    pub static_dir: String,
}

/// 统一响应信封：`code` 为数值（0=成功；WBxxxx → 数字部分），`data` 为业务数据。
#[derive(Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub code: i32,
    pub msg: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn ok(data: T) -> Json<ApiResponse<T>> {
        Json(ApiResponse {
            code: 0,
            msg: "success".into(),
            data: Some(data),
        })
    }
}

impl ApiResponse<()> {
    /// 错误响应：WB 错误码转数值（WB2004 → 2004）。
    pub fn error(code: i32, msg: impl Into<String>) -> Json<ApiResponse<()>> {
        Json(ApiResponse {
            code,
            msg: msg.into(),
            data: None,
        })
    }
}

/// WB 错误码字符串 → 数值（去掉 WB 前缀，未知 → 5000）。
fn wb_code(s: &str) -> i32 {
    s.trim_start_matches("WB").parse().unwrap_or(5000)
}

// ── 请求参数 ──────────────────────────────────────────────────────────────
#[derive(serde::Deserialize)]
struct TraceListParams {
    #[serde(rename = "runId", default)]
    run_id: Option<String>,
    #[serde(rename = "agentId", default)]
    agent_id: Option<String>,
    #[serde(rename = "objectRef", default)]
    object_ref: Option<String>,
    #[serde(rename = "eventType", default)]
    event_type: Option<String>,
    #[serde(rename = "afterId", default)]
    after_id: Option<String>,
    #[serde(rename = "pageNum", default)]
    page_num: Option<usize>,
    #[serde(rename = "pageSize", default)]
    page_size: Option<usize>,
}

#[derive(serde::Deserialize)]
struct WsParams {
    #[serde(default)]
    after: Option<String>,
}

#[derive(serde::Deserialize)]
struct ViewParam {
    #[serde(default)]
    view: Option<String>,
}

#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct RunBody {
    #[serde(rename = "agentId", default)]
    agent_id: Option<String>,
    goal: String,
    #[serde(rename = "objectRefs", default)]
    object_refs: Option<Vec<String>>,
    #[serde(default)]
    context: Option<Value>,
}

// ── 响应 DTO ─────────────────────────────────────────────────────────────
#[derive(Serialize)]
struct TraceListDto {
    total: usize,
    list: Vec<TraceEventDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RunStartDto {
    run_id: String,
    status: String,
    stream_url: String,
}

/// Memory 视图 DTO（V0.5 记忆沉淀 / BL-008）：与 `wb_memory` 同源。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MemoryRecordDto {
    label: String,
    payload: Value,
    hits: i64,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MemoryListDto {
    total: usize,
    list: Vec<MemoryRecordDto>,
}

// ── Evolution Engine（BL-003 · REQ-017）：Experience → Knowledge ────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PatternDto {
    label: String,
    action: String,
    support: i64,
    confidence: f64,
    run_ids: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProposalDto {
    id: String,
    kind: String,
    target: String,
    confidence: f64,
    status: String,
    pattern_label: String,
    run_ids: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LearnedDto {
    id: String,
    kind: String,
    action: String,
    confidence: f64,
    proposal_id: String,
    run_ids: Vec<String>,
    created_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EvolutionDto {
    patterns: Vec<PatternDto>,
    proposals: Vec<ProposalDto>,
    learned: Vec<LearnedDto>,
}

// ── 路由 ─────────────────────────────────────────────────────────────────
/// 构建路由（API 优先，静态托管兜底）。
fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/v1/wb/agent/trace", get(list_trace))
        .route("/api/v1/wb/agent/trace/:id", get(get_trace))
        .route("/api/v1/wb/agent/run/:run_id/timeline", get(timeline))
        .route("/api/v1/wb/agent/memory", get(list_memory))
        .route("/api/v1/wb/agent/evolution", get(list_evolution))
        .route("/api/v1/wb/agent/run", post(run_start))
        .route(
            "/api/v1/wb/agent/run/:run_id/approvals",
            get(list_approvals),
        )
        .route(
            "/api/v1/wb/agent/run/:run_id/approval/:req_id",
            post(resolve_approval),
        )
        .route("/api/v1/wb/agent/:id/stream", get(ws_handler))
        .fallback(get(serve_static))
        .with_state(state)
}

/// 启动 HTTP + WS 服务（绑定已就绪的 listener，返回绑定地址供测试使用）。
/// `auto_approve`：是否自动批准需审批动作（Human Approval）；
/// false → `require_approval` 动作挂起，由浏览器经 HTTP 审批端点裁决后恢复（V0.4 遗留）。
pub async fn serve(
    kernel: Arc<RuntimeKernel>,
    engine: Arc<dyn AgentEngine>,
    listener: TcpListener,
    static_dir: impl Into<String>,
    registry: ActionRegistry,
    auto_approve: bool,
) -> std::io::Result<()> {
    let state = Arc::new(AppState {
        kernel,
        engine,
        registry,
        auto_approve,
        store: Arc::new(ApprovalStore::new()),
        runs: Mutex::new(HashMap::new()),
        static_dir: static_dir.into(),
    });
    let app = router(state);
    axum::serve(listener, app).await
}

// ──  handlers  ───────────────────────────────────────────────────────────
/// `GET /api/v1/wb/agent/trace`：分页查询 Trace 事件（run/agent/object/type 过滤）。
async fn list_trace(
    State(st): State<Arc<AppState>>,
    Query(q): Query<TraceListParams>,
) -> Json<ApiResponse<TraceListDto>> {
    let tq = TraceQuery {
        run_id: q.run_id,
        agent_id: q.agent_id,
        object_ref: q.object_ref,
        event_type: q.event_type,
        after_id: q.after_id,
        limit: 1000,
    };
    let events = st.kernel.list_events(tq).await.unwrap_or_default();
    let total = events.len();
    let page = q.page_num.unwrap_or(1).max(1);
    let size = q.page_size.unwrap_or(50).clamp(1, 500);
    let start = (page - 1) * size;
    let end = (start + size).min(total);
    let list: Vec<TraceEventDto> = events[start..end]
        .iter()
        .map(|e| TraceEventDto::from(e.clone()))
        .collect();
    ApiResponse::ok(TraceListDto { total, list })
}

/// `GET /api/v1/wb/agent/trace/:id`：单条 Trace（缺失 → WB2002）。
async fn get_trace(State(st): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    match st.kernel.get_trace(&id).await {
        Ok(Some(e)) => ApiResponse::ok(TraceEventDto::from(e)).into_response(),
        Ok(None) => ApiResponse::error(wb_code("WB2002"), "Trace 不存在").into_response(),
        Err(_) => ApiResponse::error(5000, "存储错误").into_response(),
    }
}

/// `GET /api/v1/wb/agent/run/:run_id/timeline?view=`：运行时间线（三层视图）。
async fn timeline(
    State(st): State<Arc<AppState>>,
    Path(run_id): Path<String>,
    Query(q): Query<ViewParam>,
) -> Json<ApiResponse<RunTimeline>> {
    let view = View::from_param(&q.view.unwrap_or_else(|| "operator".into()));
    let events = st
        .kernel
        .list_events(TraceQuery::for_run(&run_id))
        .await
        .unwrap_or_default();
    let agent_id = events
        .first()
        .and_then(|e| e.agent_id.clone())
        .unwrap_or_default();
    let mut tl = build_timeline(events, run_id.clone(), agent_id, view);
    // 用内存态运行记录覆盖状态（捕捉 WB2004/2005 失败）
    if let Some(rec) = st.runs.lock().unwrap().get(&run_id) {
        tl.status = rec.status.as_str().to_string();
        if rec.human_approval {
            tl.human_approval = "Required".to_string();
        }
    }
    ApiResponse::ok(tl)
}

/// `GET /api/v1/wb/agent/memory`：记忆存储全量（V0.5 Memory 视图数据源，按 hits 降序）。
async fn list_memory(State(st): State<Arc<AppState>>) -> Json<ApiResponse<MemoryListDto>> {
    let records = st.kernel.recall_all().await.unwrap_or_default();
    let list = records
        .into_iter()
        .map(|m| MemoryRecordDto {
            label: m.label,
            payload: m.payload,
            hits: m.hits,
            created_at: m.created_at,
            updated_at: m.updated_at,
        })
        .collect::<Vec<_>>();
    let total = list.len();
    ApiResponse::ok(MemoryListDto { total, list })
}

/// `GET /api/v1/wb/agent/evolution`：Evolution 聚合状态（BL-003 · REQ-017）。
/// 返回 patterns / proposals / learned（含可追溯证据 run_ids 与 proposal_id）。
async fn list_evolution(State(st): State<Arc<AppState>>) -> Json<ApiResponse<EvolutionDto>> {
    let state: EvolutionState = st.kernel.evolution_state().await.unwrap_or_default();
    let patterns = state
        .patterns
        .into_iter()
        .map(|p: Pattern| PatternDto {
            label: p.label,
            action: p.action,
            support: p.support,
            confidence: p.confidence,
            run_ids: p.run_ids,
        })
        .collect::<Vec<_>>();
    let proposals = state
        .proposals
        .into_iter()
        .map(|p: Proposal| ProposalDto {
            id: p.id,
            kind: match p.kind {
                ProposalKind::ActionPattern => "ActionPattern",
            }
            .to_string(),
            target: p.target,
            confidence: p.confidence,
            status: match p.status {
                ProposalStatus::Proposed => "Proposed",
                ProposalStatus::Validated => "Validated",
                ProposalStatus::Rejected => "Rejected",
            }
            .to_string(),
            pattern_label: p.pattern_label,
            run_ids: p.run_ids,
        })
        .collect::<Vec<_>>();
    let learned = state
        .learned
        .into_iter()
        .map(|l: LearnedKnowledge| LearnedDto {
            id: l.id,
            kind: match l.kind {
                ProposalKind::ActionPattern => "ActionPattern",
            }
            .to_string(),
            action: l.action,
            confidence: l.confidence,
            proposal_id: l.proposal_id,
            run_ids: l.run_ids,
            created_at: l.created_at,
        })
        .collect::<Vec<_>>();
    ApiResponse::ok(EvolutionDto {
        patterns,
        proposals,
        learned,
    })
}

/// `GET /api/v1/wb/agent/run/:run_id/approvals`：列出该运行的全部审批请求（含历史裁决）。
async fn list_approvals(
    State(st): State<Arc<AppState>>,
    Path(run_id): Path<String>,
) -> Json<ApiResponse<Vec<ApprovalRequest>>> {
    ApiResponse::ok(st.store.list_for_run(&run_id))
}

#[derive(serde::Deserialize)]
struct ApprovalDecisionBody {
    /// `approve` | `yes` | `true` → 批准；其余 → 拒绝（WB2005 终止）。
    decision: String,
}

/// `POST /api/v1/wb/agent/run/:run_id/approval/:req_id`：裁决一条待审批请求，唤醒挂起的运行。
async fn resolve_approval(
    State(st): State<Arc<AppState>>,
    Path((_run_id, req_id)): Path<(String, String)>,
    Json(body): Json<ApprovalDecisionBody>,
) -> Response {
    let approve = matches!(
        body.decision.to_ascii_lowercase().as_str(),
        "approve" | "yes" | "true"
    );
    if st.store.resolve(&req_id, approve) {
        ApiResponse::ok(json!({
            "reqId": req_id,
            "decision": if approve { "Approved" } else { "Rejected" }
        }))
        .into_response()
    } else {
        ApiResponse::error(404, "审批请求不存在或已裁决").into_response()
    }
}

/// `POST /api/v1/wb/agent/run`：触发 Agent 运行（异步，立即返回 runId + streamUrl）。
async fn run_start(State(st): State<Arc<AppState>>, Json(body): Json<RunBody>) -> Response {
    let agent_id = match body.agent_id {
        Some(a) if !a.is_empty() => a,
        _ => return ApiResponse::error(wb_code("WB1001"), "agentId 必填").into_response(),
    };
    if body.goal.trim().is_empty() {
        return ApiResponse::error(wb_code("WB1001"), "goal 必填").into_response();
    }
    // 快速健康检查 → WB2004（避免悬挂连接，pi-doc / prd 4.5）
    if !st.engine.health().await {
        return ApiResponse::error(wb_code("WB2004"), "认知引擎不可用（Pi 未就绪）")
            .into_response();
    }

    let run_id = generate_run_id();
    st.runs.lock().unwrap().insert(
        run_id.clone(),
        RunRecord {
            status: RunStatus::Running,
            human_approval: false,
        },
    );

    let kernel = st.kernel.clone();
    let engine = st.engine.clone();
    let st2 = st.clone();
    let store = st.store.clone();
    let auto_approve = st.auto_approve;
    let goal = body.goal.clone();
    let ag = agent_id.clone();
    let rid = run_id.clone();
    tokio::spawn(async move {
        // Human Approval 闸门：
        // - auto_approve → 直接放行（AutoApprover）；
        // - 否则 `require_approval` 动作挂起，注册待审批请求并等待浏览器经 HTTP 端点裁决
        //   （ServerApprover），批准则恢复执行，拒绝则 WB2005 终止。
        let approver: Arc<dyn PolicyApprover> = if auto_approve {
            Arc::new(AutoApprover { allow: true })
        } else {
            Arc::new(ServerApprover::new(
                kernel.clone(),
                store,
                rid.clone(),
                ag.clone(),
            ))
        };
        let res = Planner::with_actions(&kernel, st.registry.clone())
            .with_approver(approver)
            .run_goal(engine.as_ref(), &goal, &ag, &rid)
            .await;
        let rec = match res {
            Ok(_) => RunRecord {
                status: RunStatus::Completed,
                human_approval: false,
            },
            Err(PlannerError::PolicyDenied(_)) => RunRecord {
                status: RunStatus::Failed,
                human_approval: true,
            },
            Err(_) => RunRecord {
                status: RunStatus::Failed,
                human_approval: false,
            },
        };
        st2.runs.lock().unwrap().insert(rid, rec);
    });

    let dto = RunStartDto {
        run_id: run_id.clone(),
        status: "Running".into(),
        stream_url: format!("/api/v1/wb/agent/{run_id}/stream"),
    };
    ApiResponse::ok(dto).into_response()
}

/// WebSocket 握手：`/api/v1/wb/agent/:id/stream`。
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(st): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(params): Query<WsParams>,
) -> Response {
    ws.on_upgrade(move |socket| handle_ws(socket, st, id, params.after))
}

/// WS 主体：先补历史（含 `?after` 重连补齐），再增量推送匹配事件。
async fn handle_ws(socket: WebSocket, st: Arc<AppState>, id: String, after: Option<String>) {
    let (mut sender, mut receiver) = socket.split();

    // 历史补齐（run/agent 任一匹配）
    let history = collect_history(&st, &id, after.as_deref()).await;
    for dto in history {
        if sender
            .send(Message::Text(serde_json::to_string(&dto).unwrap()))
            .await
            .is_err()
        {
            return;
        }
    }

    // 增量：订阅 EventBus，转发匹配事件；同时监听客户端关闭。
    let mut rx = st.kernel.event_bus().subscribe();
    loop {
        tokio::select! {
            ev = rx.recv() => {
                match ev {
                    Ok(e) => {
                        if e.run_id.as_deref() == Some(id.as_str())
                            || e.agent_id.as_deref() == Some(id.as_str())
                        {
                            let dto = TraceEventDto::from(e);
                            if sender
                                .send(Message::Text(serde_json::to_string(&dto).unwrap()))
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                    Err(_) => return, // 广播通道关闭
                }
            }
            client = receiver.next() => {
                match client {
                    Some(Ok(axum::extract::ws::Message::Close(_))) | None => return,
                    Some(Ok(_)) => continue,
                    Some(Err(_)) => return,
                }
            }
        }
    }
}

/// 收集历史事件（run 或 agent 匹配），去重排序，并按 after 游标截断（断线重连补齐）。
async fn collect_history(st: &AppState, id: &str, after: Option<&str>) -> Vec<TraceEventDto> {
    let mut run = st
        .kernel
        .list_events(TraceQuery::for_run(id))
        .await
        .unwrap_or_default();
    let mut agent = st
        .kernel
        .list_events(TraceQuery::for_agent(id))
        .await
        .unwrap_or_default();
    run.append(&mut agent);

    let mut seen: HashMap<String, crate::runtime::TraceEvent> = HashMap::new();
    for e in run {
        seen.insert(e.id.clone(), e);
    }
    let mut merged: Vec<_> = seen.into_values().collect();
    merged.sort_by(|a, b| {
        (a.timestamp.clone(), a.id.clone()).cmp(&(b.timestamp.clone(), b.id.clone()))
    });

    let cut = after.and_then(|aid| merged.iter().position(|e| e.id == aid).map(|p| p + 1));
    if let Some(start) = cut {
        merged = merged.split_off(start);
    }
    merged.into_iter().map(TraceEventDto::from).collect()
}

/// 静态文件托管（SPA 兜底 index.html）。作为 Router::fallback，保留原始请求路径。
async fn serve_static(State(st): State<Arc<AppState>>, uri: OriginalUri) -> Response {
    if uri.path().starts_with("/api/") {
        let body = ApiResponse::<()>::error(404, "接口不存在");
        return (StatusCode::NOT_FOUND, body).into_response();
    }
    let path = uri.path().trim_start_matches('/');
    let rel = if path.is_empty() {
        "index.html".to_string()
    } else {
        path.to_string()
    };
    let full = format!("{}/{}", st.static_dir, rel);
    match tokio::fs::read(&full).await {
        Ok(bytes) => (
            [(axum::http::header::CONTENT_TYPE, content_type(&full))],
            bytes,
        )
            .into_response(),
        Err(_) => {
            let idx = format!("{}/index.html", st.static_dir);
            match tokio::fs::read(&idx).await {
                Ok(b) => (
                    [(
                        axum::http::header::CONTENT_TYPE,
                        "text/html; charset=utf-8".to_string(),
                    )],
                    b,
                )
                    .into_response(),
                Err(_) => Html(
                    "<h1>OpenAIP-OS Agent Observation UI</h1>\
                     <p>未检测到 ui/dist。请进入 ui/ 执行 <code>npm install &amp;&amp; npm run build</code> 后重启服务。</p>"
                        .to_string(),
                )
                .into_response(),
            }
        }
    }
}

/// 按扩展名推断 Content-Type。
fn content_type(path: &str) -> String {
    let p = path.to_ascii_lowercase();
    if p.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if p.ends_with(".js") {
        "application/javascript; charset=utf-8"
    } else if p.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if p.ends_with(".json") {
        "application/json; charset=utf-8"
    } else if p.ends_with(".svg") {
        "image/svg+xml"
    } else if p.ends_with(".png") {
        "image/png"
    } else if p.ends_with(".ico") {
        "image/x-icon"
    } else if p.ends_with(".wasm") {
        "application/wasm"
    } else {
        "application/octet-stream"
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_start_dto_is_camel_case() {
        let dto = RunStartDto {
            run_id: "run_1".into(),
            status: "Running".into(),
            stream_url: "/api/v1/wb/agent/run_1/stream".into(),
        };
        let json = serde_json::to_value(dto).unwrap();
        assert_eq!(json["runId"], "run_1");
        assert_eq!(json["streamUrl"], "/api/v1/wb/agent/run_1/stream");
        assert!(json.get("run_id").is_none());
        assert!(json.get("stream_url").is_none());
    }
}
