//! 观察层：Trace 时间线与三层视图（对应 REQ-009 / agent-observation-design.md 4.5~4.7）
//!
//! 所有事件同源（`TraceEvent`），三层视图（Executive/Operator/Developer）仅是对同一份事件的
//! 不同裁剪；WebSocket 实时流与离线 timeline 查询共用 `TraceEventDto`（camelCase，单一真相源）。

use crate::session::run::RunStatus;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// 事件大类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraceKind {
    World,
    Cognitive,
}

impl TraceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            TraceKind::World => "world",
            TraceKind::Cognitive => "cognitive",
        }
    }
}

impl FromStr for TraceKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "world" => Ok(TraceKind::World),
            "cognitive" => Ok(TraceKind::Cognitive),
            other => Err(format!("未知 TraceKind: {other}")),
        }
    }
}

/// 统一事件（world / cognitive 同源）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TraceEvent {
    pub id: String,
    pub kind: TraceKind,
    pub event_type: String,
    /// 绑定的 Ontology 对象ID数组——可解释性根基，禁止为空（TC-007-b）。
    pub object_refs: Vec<String>,
    pub run_id: Option<String>,
    pub agent_id: Option<String>,
    pub reasoning: Option<String>,
    pub input: Option<Value>,
    pub output: Option<Value>,
    pub confidence: Option<f64>,
    pub timestamp: String,
}

static TRACE_SEQ: AtomicU64 = AtomicU64::new(0);

/// 当前微秒时间戳（Trace 事件时间轴；G1：认知层复用同一时间源）。
pub fn now_ts() -> String {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", d.as_micros())
}

/// 新 Trace 事件 ID（全局单调；G1：认知层复用同一 ID 源）。
pub fn new_trace_id() -> String {
    format!(
        "tr_{}_{}",
        now_ts(),
        TRACE_SEQ.fetch_add(1, Ordering::SeqCst)
    )
}

impl TraceEvent {
    /// 构造 world 事件（ObjectCreated/Updated/Deleted）。
    pub fn world(event_type: &str, object_ref: &str) -> Self {
        Self::world_with_refs(event_type, &[object_ref.to_string()])
    }

    /// 构造 world 事件（支持多对象绑定，如 Relation 的 [src, dst]）。
    /// [强制] object_refs 非空，否则落库被 WB2006 拦截（TC-007-b）。
    pub fn world_with_refs(event_type: &str, refs: &[String]) -> Self {
        Self {
            id: new_trace_id(),
            kind: TraceKind::World,
            event_type: event_type.to_string(),
            object_refs: refs.to_vec(),
            run_id: None,
            agent_id: None,
            reasoning: None,
            input: None,
            output: None,
            confidence: None,
            timestamp: now_ts(),
        }
    }
}

/// 认知步完整类型（用于进度计算与 Executive 筛选）。
pub const COGNITIVE_STEPS: &[&str] = &[
    "Goal",
    "Intent",
    "OntologyQuery",
    "Reasoning",
    "Planning",
    "PolicyCheck",
    "Action",
    "MemoryUpdate",
];

/// 三层视图粒度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// 管理者：仅 Mission/Progress/Decision/Risk 摘要，隐藏思考细节。
    Executive,
    /// 管理员：Pipeline 进度（Goal→…→Action），隐藏推理细节。
    Operator,
    /// 开发者：全量 Input/Output/Tool Call（类 Debugger）。
    Developer,
}

impl View {
    /// 视图名（小写，用于响应字段与服务端钳制断言）。
    pub fn name(&self) -> &'static str {
        match self {
            View::Executive => "executive",
            View::Operator => "operator",
            View::Developer => "developer",
        }
    }

    /// 由查询参数解析视图（默认 Operator）。
    pub fn from_param(s: &str) -> View {
        match s {
            "executive" => View::Executive,
            "developer" => View::Developer,
            _ => View::Operator,
        }
    }
}

/// Trace 事件 DTO（API/WS 统一载荷，camelCase；与 api-agent-trace.md 4.1 一致，禁止手写字面量）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceEventDto {
    pub id: String,
    pub kind: String,
    pub event_type: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub object_refs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    pub timestamp: String,
}

impl From<TraceEvent> for TraceEventDto {
    fn from(e: TraceEvent) -> Self {
        TraceEventDto {
            id: e.id,
            kind: e.kind.as_str().to_string(),
            event_type: e.event_type,
            object_refs: e.object_refs,
            run_id: e.run_id,
            agent_id: e.agent_id,
            reasoning: e.reasoning,
            input: e.input,
            output: e.output,
            confidence: e.confidence,
            timestamp: e.timestamp,
        }
    }
}

impl TraceEventDto {
    /// 视图感知构造（WS 实时流用）：执行服务端钳制，不信任前端自提权限。
    /// - 仅 `Developer` 视图保留 reasoning/input/output；Operator/Executive 剥除细节字段
    /// - `Executive` 视图仅保留 Goal/Action 事件（其余返回 None，调用方跳过）
    ///
    /// 与 `build_timeline` 的 REST 钳制规则保持一致（同一套最小权限策略）。
    pub fn from_view(e: &TraceEvent, view: View) -> Option<Self> {
        if matches!(view, View::Executive) && e.event_type != "Goal" && e.event_type != "Action" {
            return None;
        }
        let detail = matches!(view, View::Developer);
        Some(TraceEventDto {
            id: e.id.clone(),
            kind: e.kind.as_str().to_string(),
            event_type: e.event_type.clone(),
            object_refs: e.object_refs.clone(),
            run_id: e.run_id.clone(),
            agent_id: e.agent_id.clone(),
            reasoning: if detail { e.reasoning.clone() } else { None },
            input: if detail { e.input.clone() } else { None },
            output: if detail { e.output.clone() } else { None },
            confidence: e.confidence,
            timestamp: e.timestamp.clone(),
        })
    }
}

/// 时间线中的单个事件（视图裁剪后的形态）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineEvent {
    pub id: String,
    pub event_type: String,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub object_refs: Vec<String>,
    /// 仅 Developer 视图填充。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
}

/// Executive 视图的决策摘要（Mission/Decision/Risk）。
#[derive(Debug, Clone, Serialize)]
pub struct DecisionSummary {
    pub mission: String,
    pub decision: String,
    pub risk: String,
}

/// 运行时间线（三层视图的统一响应结构，对应 api-agent-trace.md 4.4）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunTimeline {
    pub run_id: String,
    pub agent_id: String,
    pub status: String,
    pub progress: u8,
    pub human_approval: String,
    /// 服务端钳制后的有效视图（不信任前端 `?view=`），供前端渲染对齐角色上限。
    pub view: String,
    pub events: Vec<TimelineEvent>,
    /// 仅 Executive 视图填充。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<DecisionSummary>,
}

/// 由事件列表推导运行状态（含进度与审批需求）。
/// 与前端 `ui/src/view.ts` 同源实现：失败（ActionFailed / PolicyCheck Deny）优先于 Completed；
/// 需人工审批（PolicyCheck Deny 或 PendingApproval 挂起中）→ Required。
pub fn derive_run_state(events: &[TraceEvent]) -> (RunStatus, u8, String) {
    let step_count = COGNITIVE_STEPS
        .iter()
        .filter(|s| events.iter().any(|e| e.event_type == **s))
        .count();
    let progress = ((step_count as f64 / COGNITIVE_STEPS.len() as f64) * 100.0).round() as u8;
    let denied = events.iter().any(|e| {
        e.event_type == "PolicyCheck" && e.output == Some(Value::String("Deny".to_string()))
    });
    let failed = denied || events.iter().any(|e| e.event_type == "ActionFailed");
    let has_memory = events.iter().any(|e| e.event_type == "MemoryUpdate");
    let status = if failed {
        RunStatus::Failed
    } else if has_memory {
        RunStatus::Completed
    } else {
        RunStatus::Running
    };
    let pending = events.iter().any(|e| e.event_type == "PendingApproval");
    let human_approval = if denied || pending {
        "Required"
    } else {
        "None"
    }
    .to_string();
    (status, progress, human_approval)
}

/// 由 TraceEvent 构造时间线事件（detail=true 含推理/IO，即 Developer 视图）。
fn to_timeline_event(e: &TraceEvent, detail: bool) -> TimelineEvent {
    TimelineEvent {
        id: e.id.clone(),
        event_type: e.event_type.clone(),
        timestamp: e.timestamp.clone(),
        confidence: e.confidence,
        object_refs: e.object_refs.clone(),
        reasoning: if detail { e.reasoning.clone() } else { None },
        input: if detail { e.input.clone() } else { None },
        output: if detail { e.output.clone() } else { None },
    }
}

/// 构建运行时间线并按视图裁剪（Executive 隐藏思考、Operator 仅 Pipeline、Developer 全量）。
pub fn build_timeline(
    mut events: Vec<TraceEvent>,
    run_id: String,
    agent_id: String,
    view: View,
) -> RunTimeline {
    // M8：不依赖调用方顺序，先按 (timestamp, id) 稳定排序再推导/裁剪
    // （与前端 view.ts 同源：WS 到达序可能与时间序漂移）。
    events.sort_by(|a, b| {
        (a.timestamp.clone(), a.id.clone()).cmp(&(b.timestamp.clone(), b.id.clone()))
    });
    let (status, progress, human_approval) = derive_run_state(&events);

    let (events_out, decision) = match view {
        View::Executive => {
            // 仅保留 Mission(Goal) 与 Decision/Outcome(Action)，隐藏思考细节
            let kept: Vec<TimelineEvent> = events
                .iter()
                .filter(|e| e.event_type == "Goal" || e.event_type == "Action")
                .map(|e| to_timeline_event(e, false))
                .collect();
            let mission = events
                .iter()
                .find(|e| e.event_type == "Goal")
                .and_then(|e| e.input.as_ref())
                .and_then(|v| v.get("goal"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let decision_text = events
                .iter()
                .find(|e| e.event_type == "Action")
                .map(|e| e.description_or_output())
                .unwrap_or_else(|| "无明确决策".to_string());
            let risk = events
                .iter()
                .filter(|e| e.event_type == "Reasoning")
                .filter_map(|e| e.confidence)
                .fold(0.0_f64, f64::max);
            // 风险 = 1 - 最高置信（低置信的推理才是高风险信号；与前端 view.ts 同源）。
            let risk = 1.0 - risk;
            let summary = DecisionSummary {
                mission,
                decision: decision_text,
                risk: format!("{:.2}", risk),
            };
            (kept, Some(summary))
        }
        View::Operator => {
            // 仅展示 8 个认知步骨架（每种类型取首次出现），隐藏每个候选的重复 PolicyCheck/Action 细节
            let mut seen = std::collections::HashSet::new();
            let mut all = Vec::new();
            for step in COGNITIVE_STEPS {
                if let Some(e) = events
                    .iter()
                    .find(|e| e.event_type == *step && seen.insert(e.id.clone()))
                {
                    all.push(to_timeline_event(e, false));
                }
            }
            (all, None)
        }
        View::Developer => {
            let all: Vec<TimelineEvent> =
                events.iter().map(|e| to_timeline_event(e, true)).collect();
            (all, None)
        }
    };

    RunTimeline {
        run_id,
        agent_id,
        status: status.as_str().to_string(),
        progress,
        human_approval,
        view: view.name().to_string(),
        events: events_out,
        decision,
    }
}

/// 给 TraceEvent 增加便捷方法：取 Action 的决策描述（输出或推理）。
trait DecisionText {
    fn description_or_output(&self) -> String;
}
impl DecisionText for TraceEvent {
    fn description_or_output(&self) -> String {
        self.output
            .as_ref()
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| self.reasoning.clone())
            .unwrap_or_else(|| self.event_type.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_dto_is_camel_case() {
        let tl = RunTimeline {
            run_id: "run_1".into(),
            agent_id: "retention-agent".into(),
            status: "Completed".into(),
            progress: 100,
            human_approval: "None".into(),
            view: "operator".into(),
            events: vec![TimelineEvent {
                id: "tr_1".into(),
                event_type: "Goal".into(),
                timestamp: "1".into(),
                confidence: None,
                object_refs: vec!["001".into()],
                reasoning: None,
                input: None,
                output: None,
            }],
            decision: None,
        };
        let json = serde_json::to_value(tl).unwrap();
        assert_eq!(json["runId"], "run_1");
        assert_eq!(json["agentId"], "retention-agent");
        assert_eq!(json["view"], "operator");
        assert_eq!(json["humanApproval"], "None");
        assert_eq!(json["events"][0]["eventType"], "Goal");
        assert_eq!(json["events"][0]["objectRefs"][0], "001");
        assert!(json.get("run_id").is_none());
        assert!(json.get("human_approval").is_none());
    }

    /// WS 视图钳制：`from_view` 须与 REST `build_timeline` 同一套最小权限规则。
    #[test]
    fn ws_from_view_clamps_to_role() {
        use crate::trace::{TraceEvent, TraceKind};

        // 一个带细节的推理事件（非 Goal/Action）
        let reasoning_ev = TraceEvent {
            id: "e1".into(),
            kind: TraceKind::Cognitive,
            event_type: "Reasoning".into(),
            object_refs: vec!["001".into()],
            run_id: Some("run_1".into()),
            agent_id: Some("retention-agent".into()),
            reasoning: Some("内部推理".into()),
            input: Some(serde_json::json!({"x": 1})),
            output: Some(serde_json::json!({"y": 2})),
            confidence: Some(0.9),
            timestamp: "1".into(),
        };
        // 一个 Goal 事件（Executive 允许保留，但仍应去细节）
        let goal_ev = TraceEvent {
            id: "e2".into(),
            kind: TraceKind::Cognitive,
            event_type: "Goal".into(),
            object_refs: vec!["001".into()],
            run_id: Some("run_1".into()),
            agent_id: Some("retention-agent".into()),
            reasoning: Some("目标推理".into()),
            input: Some(serde_json::json!({"goal": "g"})),
            output: Some(serde_json::json!({"ok": true})),
            confidence: None,
            timestamp: "2".into(),
        };

        // Developer：全量保留
        let dev_r = TraceEventDto::from_view(&reasoning_ev, View::Developer).unwrap();
        assert_eq!(dev_r.reasoning.as_deref(), Some("内部推理"));
        assert!(dev_r.input.is_some());
        assert!(dev_r.output.is_some());

        // Operator：剥除细节字段，但仍保留事件
        let op_r = TraceEventDto::from_view(&reasoning_ev, View::Operator).unwrap();
        assert!(op_r.reasoning.is_none());
        assert!(op_r.input.is_none());
        assert!(op_r.output.is_none());

        // Executive：非 Goal/Action 事件被丢弃
        assert!(TraceEventDto::from_view(&reasoning_ev, View::Executive).is_none());
        // Executive：Goal 事件保留但去细节
        let exec_g = TraceEventDto::from_view(&goal_ev, View::Executive).unwrap();
        assert_eq!(exec_g.event_type, "Goal");
        assert!(exec_g.reasoning.is_none());
        assert!(exec_g.output.is_none());
    }
}
