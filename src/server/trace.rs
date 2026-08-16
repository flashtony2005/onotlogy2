use std::collections::HashMap;

/// `GET /api/v1/wb/agent/trace`：分页查询 Trace 事件（run/agent/object/type 过滤）。
use std::sync::Arc;

use crate::auth::{AuthUser, clamp_view};
use crate::storage::TraceQuery;
use crate::trace::{RunTimeline, TraceEventDto, View, build_timeline};
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Extension, Path, Query, State, WebSocketUpgrade};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;

use super::*;
#[derive(serde::Deserialize)]
pub(crate) struct TraceListParams {
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
pub(crate) struct WsParams {
    #[serde(default)]
    after: Option<String>,
}

#[derive(serde::Deserialize)]
pub(crate) struct ViewParam {
    #[serde(default)]
    view: Option<String>,
}

// ── 响应 DTO ─────────────────────────────────────────────────────────────
#[derive(Serialize)]
pub(crate) struct TraceListDto {
    total: usize,
    list: Vec<TraceEventDto>,
}

pub(crate) async fn list_trace(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
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
    let events = st.storage.list_events(tq).await.unwrap_or_default();
    let total = events.len();
    let page = q.page_num.unwrap_or(1).max(1);
    let size = q.page_size.unwrap_or(50).clamp(1, 500);
    let start = (page - 1) * size;
    let end = (start + size).min(total);
    // M2：与 timeline/WS 同一套最小权限钳制——Executive 不得经列表端点读 reasoning/IO 细节。
    let view = clamp_view(View::Developer, user.role);
    let list: Vec<TraceEventDto> = events[start..end]
        .iter()
        .filter_map(|e| TraceEventDto::from_view(e, view))
        .collect();
    ApiResponse::ok(TraceListDto { total, list })
}

/// `GET /api/v1/wb/agent/trace/:id`：单条 Trace（缺失 → WB2002）。
pub(crate) async fn get_trace(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Response {
    match st.storage.get_trace(&id).await {
        Ok(Some(e)) => {
            // M2：单条查询同样按角色钳制（Executive 剥 reasoning/IO 细节）。
            let view = clamp_view(View::Developer, user.role);
            match TraceEventDto::from_view(&e, view) {
                Some(dto) => ApiResponse::ok(dto).into_response(),
                None => ApiResponse::error(404, "Trace 不在当前角色可见范围").into_response(),
            }
        }
        Ok(None) => ApiResponse::error(wb_code("WB2002"), "Trace 不存在").into_response(),
        Err(_) => ApiResponse::error(5000, "存储错误").into_response(),
    }
}

/// `GET /api/v1/wb/agent/run/:run_id/timeline?view=`：运行时间线（三层视图）。
pub(crate) async fn timeline(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(run_id): Path<String>,
    Query(q): Query<ViewParam>,
) -> Json<ApiResponse<RunTimeline>> {
    // 服务端视图钳制：前端 `?view=` 仅作建议，被钳制到角色允许上限（不信任前端自提权限）。
    let requested = View::from_param(&q.view.unwrap_or_else(|| "operator".into()));
    let view = clamp_view(requested, user.role);
    let events = st
        .storage
        .list_events(TraceQuery::for_run(&run_id))
        .await
        .unwrap_or_default();
    let agent_id = events
        .first()
        .and_then(|e| e.agent_id.clone())
        .unwrap_or_default();
    let mut tl = build_timeline(events, run_id.clone(), agent_id, view);
    // 用运行记录覆盖状态：内存优先，重启后内存为空则回退 DB（捕捉 WB2004/2005 失败）。
    if let Some(rec) = lock_mutex(&st.runs).get(&run_id) {
        tl.status = rec.status.as_str().to_string();
        if rec.human_approval {
            tl.human_approval = "Required".to_string();
        }
    } else if let Ok(Some(rec)) = st.storage.get_run(&run_id).await {
        tl.status = rec.status.clone();
        if rec.human_approval {
            tl.human_approval = "Required".to_string();
        }
    }
    ApiResponse::ok(tl)
}

/// `GET /api/v1/wb/agent/memory`：记忆存储全量（V0.5 Memory 视图数据源，按 hits 降序）。
pub(crate) async fn ws_handler(
    ws: WebSocketUpgrade,
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<String>,
    Query(params): Query<WsParams>,
) -> Response {
    // 服务端视图钳制：WS 实时流与 REST timeline 同一套最小权限策略（不信任前端自提）。
    let view = clamp_view(View::Developer, user.role);
    ws.on_upgrade(move |socket| handle_ws(socket, st, id, params.after, view))
}

/// WS 主体：先订阅缓冲（订阅与补历史之间的增量不丢），再补历史，最后增量推送匹配事件。
pub(crate) async fn handle_ws(
    socket: WebSocket,
    st: Arc<AppState>,
    id: String,
    after: Option<String>,
    view: View,
) {
    let (mut sender, mut receiver) = socket.split();

    // 先订阅再补历史：查历史期间到达的增量先入缓冲，杜绝「订阅前事件丢失」窗口
    // （前端按事件 id 去重，缓冲与历史重叠的事件会被前端幂等吸收）。
    let mut rx = st.event_tx.subscribe();
    let mut buffered: Vec<crate::trace::TraceEvent> = Vec::new();
    let mut history_fut = Box::pin(collect_history(&st, &id, after.as_deref(), view));
    let history = loop {
        tokio::select! {
            h = &mut history_fut => break h,
            ev = rx.recv() => {
                if let Ok(e) = ev
                    && (e.run_id.as_deref() == Some(id.as_str())
                        || e.agent_id.as_deref() == Some(id.as_str()))
                {
                    buffered.push(e);
                }
            }
        }
    };
    for dto in history {
        if sender
            .send(Message::Text(
                serde_json::to_string(&dto).unwrap_or_else(|_| "{}".into()),
            ))
            .await
            .is_err()
        {
            return;
        }
    }
    // 冲刷缓冲（与历史可能重叠，前端按 id 去重兜底）
    for e in buffered {
        let Some(dto) = TraceEventDto::from_view(&e, view) else {
            continue;
        };
        if sender
            .send(Message::Text(
                serde_json::to_string(&dto).unwrap_or_else(|_| "{}".into()),
            ))
            .await
            .is_err()
        {
            return;
        }
    }

    // 增量：转发匹配事件；同时监听客户端关闭。
    loop {
        tokio::select! {
            ev = rx.recv() => {
                match ev {
                    Ok(e) => {
                        if e.run_id.as_deref() == Some(id.as_str())
                            || e.agent_id.as_deref() == Some(id.as_str())
                        {
                            // 服务端视图钳制：越权细节字段剥除（Executive 仅 Goal/Action）
                            let Some(dto) = TraceEventDto::from_view(&e, view) else {
                                continue;
                            };
                            if sender
                                .send(Message::Text(serde_json::to_string(&dto).unwrap_or_else(|_| "{}".into())))
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
/// 事件经 `TraceEventDto::from_view` 按角色视图钳制后下发。
pub(crate) async fn collect_history(
    st: &AppState,
    id: &str,
    after: Option<&str>,
    view: View,
) -> Vec<TraceEventDto> {
    let mut run = st
        .storage
        .list_events(TraceQuery::for_run(id))
        .await
        .unwrap_or_default();
    let mut agent = st
        .storage
        .list_events(TraceQuery::for_agent(id))
        .await
        .unwrap_or_default();
    run.append(&mut agent);

    let mut seen: HashMap<String, crate::trace::TraceEvent> = HashMap::new();
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
    merged
        .into_iter()
        .filter_map(|e| TraceEventDto::from_view(&e, view))
        .collect()
}
