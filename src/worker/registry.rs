//! WorkerRegistry —— Worker 注册与发现（"谁来完成"）。

use std::collections::HashMap;

use super::sdk::{Worker, WorkerError};

/// Worker 状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerStatus {
    Registered,
    Active,
    Disabled,
    Unavailable,
}

/// Worker Registry。
#[derive(Default)]
pub struct WorkerRegistry {
    workers: HashMap<String, Box<dyn Worker>>,
}

impl WorkerRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册（重复 id → Err）。
    pub fn register(&mut self, worker: Box<dyn Worker>) -> Result<(), WorkerError> {
        let id = worker.manifest().id.clone();
        if self.workers.contains_key(&id) {
            return Err(WorkerError::Denied(format!("重复注册: {id}")));
        }
        self.workers.insert(id, worker);
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&dyn Worker> {
        self.workers.get(id).map(|w| w.as_ref())
    }

    /// 支持该 capability 的候选 Worker。
    pub fn candidates(&self, capability: &str) -> Vec<&dyn Worker> {
        self.workers
            .values()
            .filter(|w| w.manifest().supports(capability))
            .map(|w| w.as_ref())
            .collect()
    }

    pub fn status(&self, id: &str) -> Option<WorkerStatus> {
        self.workers.get(id).map(|_| WorkerStatus::Registered)
    }

    pub fn all(&self) -> Vec<(&str, WorkerStatus)> {
        self.workers
            .keys()
            .map(|k| (k.as_str(), WorkerStatus::Registered))
            .collect()
    }

    pub fn len(&self) -> usize {
        self.workers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.workers.is_empty()
    }
}
