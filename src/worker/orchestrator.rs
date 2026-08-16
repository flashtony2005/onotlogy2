//! WorkerOrchestrator —— Task Scheduler（Phase 11D 地基：Worker ABI 进入运行热路径）。
//!
//! OpenAIP-OS 是 Thinker / Orchestrator，不是 Worker：它理解世界、制定计划、
//! 选择 Worker、治理执行、观察结果——**不自己干具体活**。
//!
//! ```text
//! Plan（capability 步骤）
//!   → Capability → WorkerRouter → Policy Filter → Worker → Execution
//!   → Session Event Log（worker.* 全链）→ Evaluation（Phase 11A 可直接消费）
//! ```
//!
//! 与 WorkerRuntime 的边界：Runtime 负责单次 capability 执行（policy→route→execute+retry）；
//! Orchestrator 负责**整个计划**（步骤编排、结果聚合、事件流归属），并持有 run 的 SessionLog。

use serde::Serialize;
use serde_json::{Value, json};

use super::exec::WorkerRuntime;
use super::policy::WorkerPolicy;
use super::registry::WorkerRegistry;
use super::router::WorkerRouter;
use super::sdk::WorkerError;
use crate::session::event::{EventType, SessionId};
use crate::session::log::SessionLog;

/// 计划中的一步（capability 是标准单位——Skill Provider Agnostic）。
#[derive(Debug, Clone)]
pub struct WorkerTask {
    pub capability: String,
    pub input: Value,
    /// 人类可读说明（事件与报告用）。
    pub note: String,
}

impl WorkerTask {
    pub fn new(capability: &str, input: Value) -> Self {
        Self {
            capability: capability.to_string(),
            input,
            note: String::new(),
        }
    }
}

/// 单步执行结果（含归属 Worker 与状态——Router 学习的观察基础）。
#[derive(Debug, Clone, Serialize)]
pub struct TaskOutcome {
    pub capability: String,
    pub worker_id: Option<String>,
    pub status: TaskStatus,
    pub output: Option<Value>,
    pub error: Option<String>,
    pub duration_ms: u64,
}

/// 单步状态（Worker ≠ Authority：Denied/NeedsApproval 由 Policy 决定，Worker 无法绕过）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskStatus {
    Completed,
    Failed,
    Denied,
    NeedsApproval,
    NoWorker,
}

impl TaskStatus {
    pub fn status_str(&self) -> &'static str {
        match self {
            TaskStatus::Completed => "completed",
            TaskStatus::Failed => "failed",
            TaskStatus::Denied => "denied",
            TaskStatus::NeedsApproval => "needs_approval",
            TaskStatus::NoWorker => "no_worker",
        }
    }
}

/// 整个计划的执行结果。
#[derive(Debug, Clone, Serialize)]
pub struct WorkerRunResult {
    pub run_id: String,
    pub outcomes: Vec<TaskOutcome>,
    pub all_ok: bool,
}

impl WorkerRunResult {
    /// 结果性指标（供 Evaluation / Router 学习）：
    /// - success_rate：Completed / 总步数
    /// - total_duration_ms：全部完成步耗时
    pub fn success_rate(&self) -> f64 {
        if self.outcomes.is_empty() {
            return 0.0;
        }
        self.outcomes
            .iter()
            .filter(|o| o.status == TaskStatus::Completed)
            .count() as f64
            / self.outcomes.len() as f64
    }

    pub fn total_duration_ms(&self) -> u64 {
        self.outcomes.iter().map(|o| o.duration_ms).sum()
    }
}

/// Worker 计划编排器：持有 run 的 SessionLog（事实源），执行整个计划并聚合结果。
pub struct WorkerOrchestrator<'a> {
    registry: &'a WorkerRegistry,
    policy: &'a WorkerPolicy,
    router: WorkerRouter,
    log: SessionLog,
}

impl<'a> WorkerOrchestrator<'a> {
    pub fn new(
        registry: &'a WorkerRegistry,
        policy: &'a WorkerPolicy,
        router: WorkerRouter,
    ) -> Self {
        let run_id = crate::session::run::generate_run_id();
        Self {
            registry,
            policy,
            router,
            log: SessionLog::new(SessionId::new(&run_id)),
        }
    }

    pub fn run_id(&self) -> String {
        self.log.session_id.0.clone()
    }

    /// run 的 SessionLog（事实源；供 on_run_end 评测）。
    pub fn session_log(&mut self) -> &mut SessionLog {
        &mut self.log
    }

    /// 执行整个计划：每步 capability 经 Router → Policy → Worker → Event Log。
    /// 单步失败不中断后续步骤（结果全部聚合，由调用方决定处置）。
    pub async fn run_plan(&mut self, tasks: &[WorkerTask]) -> WorkerRunResult {
        let run_id = self.run_id();
        let mut outcomes = Vec::new();
        for t in tasks {
            self.log.append(
                EventType::CapabilitySelected,
                json!({ "capability": t.capability, "note": t.note }),
            );
            let started = std::time::Instant::now();
            // WorkerRuntime 临时构造：借用 self.log（方法内生命周期，无自引用问题）
            let result = {
                let mut runtime =
                    WorkerRuntime::new(self.registry, self.policy, self.router.clone())
                        .with_log(&mut self.log);
                runtime.run(&t.capability, t.input.clone()).await
            };
            let elapsed_ms = started.elapsed().as_millis() as u64;
            let worker_id = last_worker_for(&self.log, &t.capability);
            let outcome = match result {
                Ok(res) => TaskOutcome {
                    capability: t.capability.clone(),
                    worker_id,
                    status: TaskStatus::Completed,
                    output: Some(res.output),
                    error: None,
                    duration_ms: res.duration_ms.max(elapsed_ms),
                },
                Err(WorkerError::Denied(reason)) => TaskOutcome {
                    capability: t.capability.clone(),
                    worker_id: None,
                    status: TaskStatus::Denied,
                    output: None,
                    error: Some(reason),
                    duration_ms: elapsed_ms,
                },
                Err(WorkerError::RequireApproval(cap)) => TaskOutcome {
                    capability: t.capability.clone(),
                    worker_id: None,
                    status: TaskStatus::NeedsApproval,
                    output: None,
                    error: Some(format!("{cap} 需要人工审批")),
                    duration_ms: elapsed_ms,
                },
                Err(WorkerError::NoWorker(msg)) => TaskOutcome {
                    capability: t.capability.clone(),
                    worker_id: None,
                    status: TaskStatus::NoWorker,
                    output: None,
                    error: Some(msg),
                    duration_ms: elapsed_ms,
                },
                Err(WorkerError::Execution(e)) => TaskOutcome {
                    capability: t.capability.clone(),
                    worker_id,
                    status: TaskStatus::Failed,
                    output: None,
                    error: Some(e),
                    duration_ms: elapsed_ms,
                },
            };
            outcomes.push(outcome);
        }
        let all_ok = outcomes.iter().all(|o| o.status == TaskStatus::Completed);
        WorkerRunResult {
            run_id,
            outcomes,
            all_ok,
        }
    }
}

/// 从事件流定位该 capability 最近一次选中的 Worker（确定性、可回放）。
fn last_worker_for(log: &SessionLog, capability: &str) -> Option<String> {
    log.events()
        .iter()
        .rev()
        .find(|e| {
            e.event_type == EventType::WorkerSelected
                && e.data.get("capability").and_then(|v| v.as_str()) == Some(capability)
        })
        .and_then(|e| {
            e.data
                .get("worker")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
}
