//! 观察层：Trace 时间线与三层视图（对应 REQ-009 / agent-observation-design.md 4.5~4.7）
//!
//! 所有事件同源（`TraceEvent`），三层视图（Executive/Operator/Developer）仅是对同一份事件的
//! 不同裁剪；WebSocket 实时流与离线 timeline 查询共用 `TraceEventDto`（camelCase，单一真相源）。

use crate::planner::RunStatus;
use crate::runtime::TraceEvent;
use serde::Serialize;
use serde_json::Value;

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

/// 时间线中的单个事件（视图裁剪后的形态）。
#[derive(Debug, Clone, Serialize)]
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
pub struct RunTimeline {
    pub run_id: String,
    pub agent_id: String,
    pub status: String,
    pub progress: u8,
    pub human_approval: String,
    pub events: Vec<TimelineEvent>,
    /// 仅 Executive 视图填充。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<DecisionSummary>,
}

/// 由事件列表推导运行状态（含进度与审批需求）。
pub fn derive_run_state(events: &[TraceEvent]) -> (RunStatus, u8, String) {
    let step_count = COGNITIVE_STEPS
        .iter()
        .filter(|s| events.iter().any(|e| e.event_type == **s))
        .count();
    let progress = ((step_count as f64 / COGNITIVE_STEPS.len() as f64) * 100.0).round() as u8;
    let has_memory = events.iter().any(|e| e.event_type == "MemoryUpdate");
    let status = if has_memory {
        RunStatus::Completed
    } else {
        RunStatus::Running
    };
    let denied = events.iter().any(|e| {
        e.event_type == "PolicyCheck" && e.output == Some(Value::String("Deny".to_string()))
    });
    let human_approval = if denied { "Required" } else { "None" }.to_string();
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
    events: Vec<TraceEvent>,
    run_id: String,
    agent_id: String,
    view: View,
) -> RunTimeline {
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
