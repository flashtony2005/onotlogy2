//! StateStore —— Context 作用域内的 KV 状态（「我在哪里」的世界状态一部分）。
//!
//! Phase 0：内存 HashMap；Phase 1 由持久化层接管（contexts.state 列）。

use serde_json::Value;
use std::collections::HashMap;

/// Context 作用域 KV（Entity 的当前状态可在上层由 Relation + State 组合定义）。
#[derive(Debug, Clone, Default)]
pub struct StateStore {
    map: HashMap<String, Value>,
}

impl StateStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, key: impl Into<String>, value: Value) {
        self.map.insert(key.into(), value);
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.map.get(key)
    }

    pub fn remove(&mut self, key: &str) -> Option<Value> {
        self.map.remove(key)
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.map.keys()
    }

    /// 全量快照（持久化用）。
    pub fn snapshot(&self) -> Value {
        serde_json::to_value(&self.map).unwrap_or(Value::Null)
    }

    /// 从快照恢复。
    pub fn from_snapshot(v: Value) -> Self {
        let map = v
            .as_object()
            .map(|o| o.iter().map(|(k, val)| (k.clone(), val.clone())).collect());
        Self {
            map: map.unwrap_or_default(),
        }
    }
}
