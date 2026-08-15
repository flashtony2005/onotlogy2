//! Memory —— Agent 认知状态（P4 / D3：不进 fact 层，Phase 3 仅内存态）。
//!
//! 沉淀：Effect 成功提交后 `record(action)`（hits 累加，V1 wb_memory 语义映射）。
//! 召回：决策前 `recall_all()` 注入 AgentView。
//! 持久化：Phase 4（Temporal World）统一处理。

use serde_json::Value;
use std::collections::HashMap;

/// 一条记忆。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryItem {
    pub label: String,
    pub payload: Value,
    pub hits: u64,
}

/// Agent 记忆（认知状态）。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Memory {
    items: HashMap<String, MemoryItem>,
}

impl Memory {
    pub fn new() -> Self {
        Self::default()
    }

    /// 沉淀（hits 累加，payload 更新）。
    pub fn record(&mut self, label: impl Into<String>, payload: Value) {
        let label = label.into();
        match self.items.get_mut(&label) {
            Some(m) => {
                m.hits += 1;
                m.payload = payload;
            }
            None => {
                self.items.insert(
                    label.clone(),
                    MemoryItem {
                        label,
                        payload,
                        hits: 1,
                    },
                );
            }
        }
    }

    /// 单条召回。
    pub fn recall(&self, label: &str) -> Option<&MemoryItem> {
        self.items.get(label)
    }

    /// 全量召回（决策前注入 AgentView）。
    pub fn recall_all(&self) -> Vec<MemoryItem> {
        self.items.values().cloned().collect()
    }

    /// 动作记忆（hits 语义，供决策引擎查询）。
    pub fn hits_of(&self, label: &str) -> u64 {
        self.items.get(label).map(|m| m.hits).unwrap_or(0)
    }
}
