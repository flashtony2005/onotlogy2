//! 出世界能力缝：可替换 Provider + 流水线闸门。
//!
//! 本体（ODL）声明对象能做什么；Planner 编排；本模块只负责
//! 「怎么执行出世界副作用」以及 pre/post 闸门。ActionEngine 仍只写世界。

use crate::storage::ObjectRecord;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

/// 后端执行失败（不映射为 Policy Deny）。
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("工具后端失败: {0}")]
    Failed(String),
}

/// 后端产出：额外参数并入动作 args；`skip_world` 时不跑 ActionEngine。
#[derive(Debug, Clone, Default)]
pub struct ToolOutcome {
    pub extra_args: HashMap<String, Value>,
    pub skip_world: bool,
}

/// 可替换的出世界执行器。不支持的动作返回 `supports=false`，Planner 只跑本体效应。
#[async_trait]
pub trait ToolBackend: Send + Sync {
    fn name(&self) -> &str;
    fn supports(&self, object_type: &str, action: &str) -> bool;
    async fn execute(
        &self,
        object_type: &str,
        action: &str,
        target: &ObjectRecord,
        args: &HashMap<String, Value>,
    ) -> Result<ToolOutcome, ToolError>;
}

/// 默认：不接管任何动作。
pub struct NullBackend;

#[async_trait]
impl ToolBackend for NullBackend {
    fn name(&self) -> &str {
        "null"
    }
    fn supports(&self, _object_type: &str, _action: &str) -> bool {
        false
    }
    async fn execute(
        &self,
        _object_type: &str,
        _action: &str,
        _target: &ObjectRecord,
        _args: &HashMap<String, Value>,
    ) -> Result<ToolOutcome, ToolError> {
        Ok(ToolOutcome::default())
    }
}

/// 流水线闸门：pre 返回 Err 则本步 Deny（不执行后端/世界）。
#[async_trait]
pub trait ActionHook: Send + Sync {
    fn name(&self) -> &str;
    async fn pre(
        &self,
        object_type: &str,
        action: &str,
        target_id: &str,
    ) -> Result<(), String> {
        let _ = (object_type, action, target_id);
        Ok(())
    }
    async fn post(
        &self,
        object_type: &str,
        action: &str,
        target_id: &str,
        outcome: &ToolOutcome,
    ) {
        let _ = (object_type, action, target_id, outcome);
    }
}
