//! V2 API —— Skill / Plugin / Session 只读面板 + Skill Import。
//!
//! 认知状态容器（内存 + 可选持久化）：skills / plugins / sessions。

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use axum::Json;
use axum::extract::{Path, State};
use serde::Serialize;
use tokio::sync::Mutex;

use crate::plugin::registry::PluginRegistry;
use crate::session::event::{EventType, SessionId};
use crate::session::log::SessionLog;
use crate::skill::registry::SkillRegistry;

/// V2 认知状态容器（挂载到 AppState）。
#[derive(Default)]
pub struct V2State {
    pub skills: RwLock<SkillRegistry>,
    pub plugins: RwLock<PluginRegistry>,
    pub sessions: Mutex<HashMap<String, SessionLog>>,
}

pub type SharedV2 = Arc<V2State>;

// ── DTO ────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SkillDto {
    pub id: String,
    pub name: String,
    pub version: String,
    pub skill_type: String,
    pub status: String,
    pub goal: String,
    pub capabilities: Vec<String>,
}

#[derive(Serialize)]
pub struct PluginDto {
    pub id: String,
    pub name: String,
    pub status: String,
    pub runtime: String,
    pub endpoint: Option<String>,
    pub capabilities: Vec<String>,
}

#[derive(Serialize)]
pub struct SessionEventDto {
    pub seq: u64,
    pub event_type: String,
    pub timestamp: i64,
    pub data: serde_json::Value,
}

#[derive(Serialize)]
pub struct SessionDto {
    pub session_id: String,
    pub events: Vec<SessionEventDto>,
}

#[derive(serde::Deserialize)]
pub struct ImportSkillReq {
    pub name: String,
    pub content: String,
    /// 已知能力集（调用方提供；未列出的能力只产生 Candidate——P4）。
    #[serde(default)]
    pub known_capabilities: Vec<String>,
}

// ── Handlers ───────────────────────────────────────────────────────────

pub async fn list_skills(State(state): State<Arc<crate::server::AppState>>) -> Json<Vec<SkillDto>> {
    let v2 = &state.v2;
    let reg = v2.skills.read().unwrap();
    let mut out: Vec<SkillDto> = reg
        .all()
        .into_iter()
        .map(|a| SkillDto {
            id: a.id.clone(),
            name: a.name.clone(),
            version: a.version.clone(),
            skill_type: a.skill_type.as_str().to_string(),
            status: format!("{:?}", a.status),
            goal: a.goal.clone(),
            capabilities: a.capabilities.clone(),
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Json(out)
}

/// Import：文本 → 解析 → Resolve（跳过未注册能力）→ Compile → 注册。
pub async fn import_skill(
    State(state): State<Arc<crate::server::AppState>>,
    Json(req): Json<ImportSkillReq>,
) -> Result<Json<SkillDto>, (axum::http::StatusCode, String)> {
    let v2 = &state.v2;
    use crate::skill::asset::AssetCompiler;
    use crate::skill::parser::{AISemanticParser, MarkdownSkillParser};
    use crate::skill::raw::RawSkill;
    use crate::skill::resolver::SkillResolver;

    let raw = RawSkill::import(&req.name, &req.content);
    let ir = MarkdownSkillParser
        .parse(&raw)
        .map_err(|e| (axum::http::StatusCode::BAD_REQUEST, e.to_string()))?;
    let known: Vec<String> = {
        let reg = v2.skills.read().unwrap();
        // 已知能力：调用方提供 + 已注册 asset 的 capabilities 并集
        let mut k = req.known_capabilities.clone();
        for a in reg.all() {
            for c in &a.capabilities {
                if !k.contains(c) {
                    k.push(c.clone());
                }
            }
        }
        k
    };
    let resolutions = SkillResolver::new(&known).resolve_capabilities(&ir);
    let asset = AssetCompiler::default()
        .compile(&raw, &ir, &resolutions)
        .map_err(|e| (axum::http::StatusCode::BAD_REQUEST, e.to_string()))?;
    let mut reg = v2.skills.write().unwrap();
    reg.attach_raw(&raw.id, raw.clone());
    reg.register(asset.clone())
        .map_err(|e| (axum::http::StatusCode::CONFLICT, e.to_string()))?;
    Ok(Json(SkillDto {
        id: asset.id.clone(),
        name: asset.name.clone(),
        version: asset.version.clone(),
        skill_type: asset.skill_type.as_str().to_string(),
        status: format!("{:?}", asset.status),
        goal: asset.goal.clone(),
        capabilities: asset.capabilities.clone(),
    }))
}

/// 原始内容 DTO（P1：Raw 不修改，只读展示）。
#[derive(Serialize)]
pub struct RawSkillDto {
    pub id: String,
    pub name: String,
    pub source_type: String,
    pub content_hash: String,
    pub content: String,
}

/// GET /api/v1/v2/skills/:id/raw —— 查看 Skill 的原始内容。
pub async fn get_skill_raw(
    State(state): State<Arc<crate::server::AppState>>,
    Path(id): Path<String>,
) -> Result<Json<RawSkillDto>, (axum::http::StatusCode, String)> {
    let reg = state.v2.skills.read().unwrap();
    let raw = reg.raw_of(&id).ok_or((
        axum::http::StatusCode::NOT_FOUND,
        format!("skill {id} 无原始快照"),
    ))?;
    Ok(Json(RawSkillDto {
        id: raw.id.clone(),
        name: raw.name.clone(),
        source_type: format!("{:?}", raw.source_type),
        content_hash: raw.content_hash.clone(),
        content: raw.content.clone(),
    }))
}

pub async fn list_plugins(
    State(state): State<Arc<crate::server::AppState>>,
) -> Json<Vec<PluginDto>> {
    let v2 = &state.v2;
    let reg = v2.plugins.read().unwrap();
    let mut out: Vec<PluginDto> = reg
        .snapshot()
        .plugins
        .into_iter()
        .map(|(m, st)| PluginDto {
            id: m.id.clone(),
            name: m.name.clone(),
            status: format!("{st:?}"),
            runtime: format!("{:?}", m.runtime),
            endpoint: m.endpoint.clone(),
            capabilities: m.capabilities.iter().map(|c| c.id.clone()).collect(),
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out.dedup_by(|a, b| a.id == b.id);
    Json(out)
}

pub async fn get_session(
    State(state): State<Arc<crate::server::AppState>>,
    Path(id): Path<String>,
) -> Result<Json<SessionDto>, (axum::http::StatusCode, String)> {
    let sessions = state.v2.sessions.lock().await;
    let log = sessions.get(&id).ok_or((
        axum::http::StatusCode::NOT_FOUND,
        format!("session {id} 不存在"),
    ))?;
    Ok(Json(SessionDto {
        session_id: log.session_id.0.clone(),
        events: log
            .events()
            .iter()
            .map(|e| SessionEventDto {
                seq: e.seq,
                event_type: e.event_type.as_str().to_string(),
                timestamp: e.timestamp,
                data: e.data.clone(),
            })
            .collect(),
    }))
}

pub async fn create_session(
    State(state): State<Arc<crate::server::AppState>>,
    Path(id): Path<String>,
) -> Json<SessionDto> {
    let mut sessions = state.v2.sessions.lock().await;
    let log = sessions
        .entry(id.clone())
        .or_insert_with(|| SessionLog::new(SessionId::new(&id)));
    log.append(EventType::SessionCreated, serde_json::json!({}));
    Json(SessionDto {
        session_id: id,
        events: log
            .events()
            .iter()
            .map(|e| SessionEventDto {
                seq: e.seq,
                event_type: e.event_type.as_str().to_string(),
                timestamp: e.timestamp,
                data: e.data.clone(),
            })
            .collect(),
    })
}

/// Session 概览（dashboard 用：id + 事件数）。
#[derive(Serialize)]
pub struct SessionSummaryDto {
    pub session_id: String,
    pub events: usize,
}

pub async fn list_sessions(
    State(state): State<Arc<crate::server::AppState>>,
) -> Json<Vec<SessionSummaryDto>> {
    let sessions = state.v2.sessions.lock().await;
    let mut out: Vec<SessionSummaryDto> = sessions
        .iter()
        .map(|(id, log)| SessionSummaryDto {
            session_id: id.clone(),
            events: log.len(),
        })
        .collect();
    out.sort_by(|a, b| b.session_id.cmp(&a.session_id));
    Json(out)
}
