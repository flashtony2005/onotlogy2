#!/usr/bin/env python3
"""server.rs → server/ 目录切分（纯机械搬移，行为零变化）。"""
import os, shutil

SRC = "src/server.rs"
OUT = "src/server"
lines = open(SRC).read().split("\n")

def seg(a, b):  # [a, b) 1-based
    block = lines[a - 1 : b - 1]
    while block and block[-1].strip() == "":
        block.pop()
    return "\n".join(block)

# mod.rs：状态/基础设施/DTO 路由（router 引用子模块 handler，需在子模块后声明 use）
mod_parts = [
    seg(43, 129),     # RunRecord / AppState / LoginFail / consts / impl AppState
    seg(129, 172),    # ApiResponse / wb_code / now_micros（#[derive] 归 trace）
    seg(314, 554),    # router / serve / serve_with_timeout / recover / security_headers / auth_layer / helpers / AuthToken
]

# auth.rs
auth_parts = [seg(554, 617)]  # LoginBody + auth_login/logout/me

# graph.rs
graph_parts = [seg(617, 662)]

# trace.rs：参数 DTO + 事件端点 + WS
trace_parts = [seg(172, 202), seg(214, 220), seg(662, 746), seg(1166, 1309)]  # TraceListParams + TraceListDto(含注释/derive) + handlers + WS

# views.rs：业务 DTO + memory/tasks/evolution 端点
views_parts = [seg(229, 314), seg(746, 986)]  # 含 MemoryRecordDto 注释/derive

# approvals.rs
approvals_parts = [seg(986, 1068)]

# run.rs
run_parts = [seg(202, 214), seg(221, 228), seg(1068, 1166), seg(1374, len(lines) + 1)]  # RunBody + RunStartDto + run_start + tests

# static.rs
static_parts = [seg(1309, 1374)]

DOC = """//! HTTP + WS 服务层（对应 REQ-009 / agent-observation-design.md）
//!
//! 模块拆分（长文件模块化）：状态/路由/鉴权中间件在 `mod`，认证端点 `auth`，
//! 图端点 `graph`，事件/WS `trace`，业务视图 `views`，审批 `approvals`，
//! 运行触发 `run`，静态托管 `static`。

"""

MOD_HEAD = """pub mod approvals;
pub mod auth;
pub mod graph;
pub mod run;
pub mod static_files;
pub mod trace;
pub mod views;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::action::ActionRegistry;
use crate::agent::AgentEngine;
use crate::auth::{AuthUser, IdentityProvider, LocalIdentityProvider, Role, SessionStore};
use crate::planner::{ApprovalStore, RunStatus, lock_mutex};
use crate::runtime::RuntimeKernel;
use axum::extract::{Extension, OriginalUri, Path, Query, Request, State, WebSocketUpgrade};
use axum::http::{StatusCode, header::AUTHORIZATION};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Serialize;
use tokio::net::TcpListener;

// router 引用的子模块 handler
use approvals::list_approvals;
use approvals::resolve_approval;
use auth::auth_login;
use auth::auth_logout;
use auth::auth_me;
use graph::list_graph;
use run::run_start;
use static_files::serve_static;
use trace::collect_history;
use trace::get_trace;
use trace::handle_ws;
use trace::list_trace;
use trace::timeline;
use trace::ws_handler;
use views::list_approval_audit_handler;
use views::list_evolution;
use views::list_memory;
use views::list_tasks;
use views::revoke_learned;
use views::toggle_auto_approve;

"""

AUTH_HEAD = """use std::sync::Arc;

use crate::auth::AuthUser;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::Json;
use serde_json::json;

use super::*;
"""

GRAPH_HEAD = """use std::sync::Arc;

use crate::auth::{AuthUser, Role};
use axum::extract::{Extension, State};
use axum::response::Response;
use serde_json::{Value, json};

use super::*;
"""

TRACE_HEAD = """use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::auth::{AuthUser, clamp_view};
use crate::runtime::TraceEvent as RtTraceEvent;
use crate::planner::lock_mutex;
use crate::storage::TraceQuery;
use crate::trace::{RunTimeline, TraceEventDto, View, build_timeline};
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Extension, Path, Query, State, WebSocketUpgrade};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::json;
use tokio::net::TcpListener;

use super::*;
"""

VIEWS_HEAD = """use std::sync::Arc;

use crate::auth::AuthUser;
use crate::evolution::{EvolutionState, LearnedKnowledge, Pattern, Proposal, ProposalKind, ProposalStatus};
use crate::storage::ApprovalAudit;
use axum::extract::{Extension, State};
use axum::response::Response;
use serde::Serialize;
use serde_json::{Value, json};

use super::*;
"""

APPROVALS_HEAD = """use std::collections::HashMap;
use std::sync::Arc;

use crate::auth::AuthUser;
use crate::planner::{ApprovalRequest, ApprovalState, ApprovalStore};
use crate::storage::ApprovalRecord;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::{Value, json};

use super::*;
"""

RUN_HEAD = """use std::sync::Arc;

use crate::auth::AuthUser;
use crate::planner::{AutoApprover, Planner, PlannerError, PolicyApprover, RunStatus, RuntimeConfig, ServerApprover, generate_run_id, lock_mutex};
use crate::runtime::RuntimeKernel;
use serde_json::Value;
use axum::extract::{Extension, State};
use axum::response::Response;
use axum::Json;
use serde::Serialize;
use serde_json::json;

use super::*;
"""

STATIC_HEAD = """use std::sync::Arc;

use axum::extract::{OriginalUri, State};
use axum::response::{Html, IntoResponse, Response};
use axum::Json;
use serde_json::json;

use super::*;
"""

os.makedirs(OUT, exist_ok=True)

with open(f"{OUT}/mod.rs", "w") as f:
    f.write(DOC + MOD_HEAD + "\n\n".join(mod_parts) + "\n")
with open(f"{OUT}/auth.rs", "w") as f:
    f.write(AUTH_HEAD + "\n\n".join(auth_parts) + "\n")
with open(f"{OUT}/graph.rs", "w") as f:
    f.write(GRAPH_HEAD + "\n\n".join(graph_parts) + "\n")
with open(f"{OUT}/trace.rs", "w") as f:
    f.write(TRACE_HEAD + "\n\n".join(trace_parts) + "\n")
with open(f"{OUT}/views.rs", "w") as f:
    f.write(VIEWS_HEAD + "\n\n".join(views_parts) + "\n")
with open(f"{OUT}/approvals.rs", "w") as f:
    f.write(APPROVALS_HEAD + "\n\n".join(approvals_parts) + "\n")
with open(f"{OUT}/run.rs", "w") as f:
    f.write(RUN_HEAD + "\n\n".join(run_parts) + "\n")
with open(f"{OUT}/static_files.rs", "w") as f:
    f.write(STATIC_HEAD + "\n\n".join(static_parts) + "\n")

VIS = {
    "auth.rs": ['async fn auth_login', 'async fn auth_logout', 'async fn auth_me', 'struct LoginBody'],
    "graph.rs": ['async fn list_graph'],
    "trace.rs": ['async fn list_trace', 'async fn get_trace', 'async fn timeline', 'async fn ws_handler', 'async fn handle_ws', 'async fn collect_history', 'struct TraceListParams', 'struct ViewParam', 'struct WsParams', 'struct TraceListDto'],
    "views.rs": ['async fn list_memory', 'async fn list_tasks', 'async fn list_evolution', 'async fn revoke_learned', 'async fn toggle_auto_approve', 'async fn list_approval_audit_handler', 'struct AutoApproveBody'],
    "approvals.rs": ['async fn list_approvals', 'async fn resolve_approval', 'struct ApprovalDecisionBody'],
    "run.rs": ['async fn run_start', 'struct RunBody', 'struct RunStartDto'],
    "static_files.rs": ['async fn serve_static'],
}

for fname, fns in VIS.items():
    p = f"{OUT}/{fname}"
    body = open(p).read()
    for item in fns:
        if item.startswith("async fn "):
            body = body.replace(item, item.replace("async fn ", "pub(crate) async fn ", 1), 1)
        elif item.startswith("struct "):
            body = body.replace(item, item.replace("struct ", "pub(crate) struct ", 1), 1)
    open(p, "w").write(body)
shutil.move(SRC, SRC + ".bak")

# ── 后处理：段尾孤立 doc（下一符号的属性）归位到下一个文件头部 ──
ORDER = ["mod.rs", "auth.rs", "graph.rs", "trace.rs", "views.rs", "approvals.rs", "run.rs", "static_files.rs"]

def strip_tail_docs(path):
    ls = open(path).read().split("\n")
    i = len(ls) - 1
    docs = []
    while i >= 0 and (ls[i].strip().startswith("///") or ls[i].strip() == ""):
        if ls[i].strip().startswith("///"):
            docs.append(ls[i])
        i -= 1
    if docs:
        open(path, "w").write("\n".join(ls[: i + 1]).rstrip() + "\n")
    return docs[::-1]

for idx, fname in enumerate(ORDER):
    path = f"{OUT}/{fname}"
    docs = strip_tail_docs(path)
    if docs and idx + 1 < len(ORDER):
        nxt = f"{OUT}/{ORDER[idx + 1]}"
        d = open(nxt).read()
        # 插到头部（第一个 `use` 段后 / 文件头注释后）
        anchor = d.find("use ")
        insert_at = d.find("\n", anchor) + 1 if anchor >= 0 else 0
        d = d[:insert_at] + "\n" + "\n".join(docs) + "\n" + d[insert_at:]
        open(nxt, "w").write(d)

print("server split done")
