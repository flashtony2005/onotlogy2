//! CompensationExecutor —— 错误恢复执行器（D4 分离）。
//!
//! Compensation 是错误恢复语义：撤销正向 Effect 产生的世界变化。
//! Phase 5 MVP：RelationCompensationExecutor 闭合（expire）关系时间窗。

use crate::kernel::{ContextId, Kernel};

/// 补偿错误（→ CompensationEngine 升级人工 ESCALATED）。
#[derive(Debug, thiserror::Error)]
pub enum CompensationError {
    #[error("补偿失败: {0}")]
    Failed(String),
}

/// 补偿结果。
#[derive(Debug, Clone, Default)]
pub struct CompensationResult {
    /// 已撤销的关系数。
    pub undone: usize,
}

/// 补偿执行器（与 EffectExecutor 分离：错误恢复语义）。
pub trait CompensationExecutor: Send + Sync {
    fn compensate(
        &mut self,
        compensation: &crate::kernel::Compensation,
    ) -> Result<CompensationResult, CompensationError>;
}

/// V2-native 关系补偿执行器：闭合 compensation.payload 中声明的关系时间窗。
///
/// payload 约定：`{ "relations": [ [source, predicate, target], ... ] }`
/// （由正向执行的 EffectResult 反向生成）。
pub struct RelationCompensationExecutor<'k> {
    kernel: &'k mut Kernel,
}

impl<'k> RelationCompensationExecutor<'k> {
    pub fn new(kernel: &'k mut Kernel) -> Self {
        Self { kernel }
    }
}

impl CompensationExecutor for RelationCompensationExecutor<'_> {
    fn compensate(
        &mut self,
        compensation: &crate::kernel::Compensation,
    ) -> Result<CompensationResult, CompensationError> {
        let rels: Vec<Vec<String>> = compensation
            .payload
            .get("relations")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        if rels.is_empty() {
            // 无撤销清单：按 action 全量闭合该 context 的同谓词关系（尽力恢复）
            let ctx = compensation
                .payload
                .get("context")
                .and_then(|v| v.as_str())
                .and_then(|s| s.strip_prefix("ctx_"))
                .and_then(|n| n.parse::<u64>().ok())
                .map(ContextId);
            if let Some(c) = ctx {
                return self.undo_by_action(c, &compensation.action);
            }
            return Ok(CompensationResult::default());
        }
        let mut undone = 0;
        for triple in rels {
            if triple.len() < 3 {
                continue;
            }
            let ids: Vec<crate::kernel::RelationId> = self
                .kernel
                .relations()
                .filter(|r| {
                    r.source == triple[0] && r.predicate == triple[1] && r.target == triple[2]
                })
                .map(|r| r.id)
                .collect();
            for id in ids {
                let _ = self.kernel.expire_relation(id, self.kernel.now());
                undone += 1;
            }
        }
        Ok(CompensationResult { undone })
    }
}

impl RelationCompensationExecutor<'_> {
    /// 按 action 语义：闭合该 context 内由该动作产生的全部关系（如 retain 的 needs_retention）。
    fn undo_by_action(
        &mut self,
        ctx: ContextId,
        action: &str,
    ) -> Result<CompensationResult, CompensationError> {
        // 匹配 source 含 target 前缀的关系（V2 约定：动作产生的关系 source = target）
        let ids: Vec<crate::kernel::RelationId> = self
            .kernel
            .relations()
            .filter(|r| r.context == ctx && r.target.starts_with(&format!("{action}_")))
            .map(|r| r.id)
            .collect();
        let mut undone = 0;
        for id in ids {
            let _ = self.kernel.expire_relation(id, self.kernel.now());
            undone += 1;
        }
        Ok(CompensationResult { undone })
    }
}
