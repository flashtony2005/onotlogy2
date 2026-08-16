//! WorkerRuntime —— Capability → Policy → Worker Selection → Execution → Result。
//!
//! 全链路审计（Runtime-visible ⇒ Logged）：
//! policy.checked → worker.selected → worker.started → worker.completed / worker.failed
//! 失败自动重试一次（R10）。

use serde_json::{Value, json};

use super::policy::{WorkerPolicy, WorkerVerdict};
use super::registry::WorkerRegistry;
use super::router::WorkerRouter;
use super::sdk::{WorkerError, WorkerResult};
use crate::session::event::EventType;
use crate::session::log::SessionLog;

/// Worker 运行时。
pub struct WorkerRuntime<'a> {
    registry: &'a WorkerRegistry,
    policy: &'a WorkerPolicy,
    router: WorkerRouter,
    log: Option<&'a mut SessionLog>,
    /// 失败重试次数（默认 1）。
    pub max_retries: u32,
}

impl<'a> WorkerRuntime<'a> {
    pub fn new(
        registry: &'a WorkerRegistry,
        policy: &'a WorkerPolicy,
        router: WorkerRouter,
    ) -> Self {
        Self {
            registry,
            policy,
            router,
            log: None,
            max_retries: 1,
        }
    }

    pub fn with_log(mut self, log: &'a mut SessionLog) -> Self {
        self.log = Some(log);
        self
    }

    fn emit(&mut self, event_type: EventType, data: Value) {
        if let Some(log) = &mut self.log {
            log.append(event_type, data);
        }
    }

    /// 执行链：Policy → Router → Execute（失败重试一次）。
    pub async fn run(
        &mut self,
        capability: &str,
        input: Value,
    ) -> Result<WorkerResult, WorkerError> {
        // 1) Policy（fail-closed）
        match self.policy.check(capability, &input) {
            WorkerVerdict::Deny(reason) => {
                self.emit(
                    EventType::PolicyChecked,
                    json!({ "capability": capability, "verdict": "deny", "reason": reason }),
                );
                return Err(WorkerError::Denied(reason));
            }
            WorkerVerdict::RequireApproval => {
                self.emit(
                    EventType::PolicyChecked,
                    json!({ "capability": capability, "verdict": "require_approval" }),
                );
                self.emit(
                    EventType::ApprovalRequested,
                    json!({ "capability": capability }),
                );
                return Err(WorkerError::RequireApproval(capability.to_string()));
            }
            WorkerVerdict::Allow => {
                self.emit(
                    EventType::PolicyChecked,
                    json!({ "capability": capability, "verdict": "allow" }),
                );
            }
        }
        // 2) Router
        let sel = self
            .router
            .route(self.registry, capability, self.policy)
            .inspect_err(|e| {
                self.emit(
                    EventType::Error,
                    json!({ "capability": capability, "message": e.to_string() }),
                );
            })?;
        self.emit(
            EventType::WorkerSelected,
            json!({
                "worker": sel.worker_id,
                "capability": sel.capability,
                "reason": sel.reason,
                "policy_verdict": sel.policy_verdict,
                "effective_trust": sel.effective_trust,
                "candidates": sel.candidates.iter().map(|(id, score)| json!({
                    "worker": id,
                    "trust_score": score,
                })).collect::<Vec<_>>(),
            }),
        );
        // 3) Execute（失败重试 max_retries 次）
        let worker = self
            .registry
            .get(&sel.worker_id)
            .ok_or(WorkerError::NoWorker(sel.worker_id.clone()))?;
        let mut attempts = 0;
        loop {
            attempts += 1;
            self.emit(
                EventType::WorkerStarted,
                json!({ "worker": sel.worker_id, "capability": capability, "attempt": attempts }),
            );
            match worker.execute(capability, input.clone()).await {
                Ok(result) => {
                    self.emit(
                        EventType::WorkerCompleted,
                        json!({
                            "worker": sel.worker_id,
                            "capability": capability,
                            "ok": result.ok,
                            "duration_ms": result.duration_ms,
                        }),
                    );
                    return Ok(result);
                }
                Err(e) => {
                    self.emit(
                        EventType::WorkerFailed,
                        json!({
                            "worker": sel.worker_id,
                            "capability": capability,
                            "attempt": attempts,
                            "error": e.to_string(),
                        }),
                    );
                    if attempts <= self.max_retries {
                        continue; // 重试
                    }
                    return Err(e);
                }
            }
        }
    }
}
