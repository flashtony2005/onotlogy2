//! EffectExecutor —— 正向世界变化执行器（D4 分离：Effect 语义 ≠ Compensation 语义）。
//!
//! `Kernel::emit` 只登记 Effect，不修改 World；本模块是 Agent Task 把 Effect
//! 应用到 Kernel World relations 的唯一正向执行边界。`EffectResult` 仅返回本次
//! 已执行的关系变化，供调用方写审计事件和构造补偿清单，调用方不得再次 apply。
//!
//! 这里的正向执行不同于 `crate::projection::Projection`：执行器改变当前
//! Kernel World 状态，投影器只从已存在的 World Event 重建派生索引。

use serde_json::Value;

use crate::kernel::relation::Relation;
use crate::kernel::{Kernel, KernelError};

/// 执行错误（→ Fiber 失败，触发 CompensationEngine）。
#[derive(Debug, thiserror::Error)]
pub enum EffectError {
    #[error("效果执行失败: {0}")]
    Failed(String),
    #[error("未找到动作的效应声明: {0}")]
    NoEffects(String),
}

/// 执行结果：已由执行器应用的真实世界变化（关系集）。
///
/// 调用方可用它写审计事件或补偿清单，但不应再次写入 Kernel relations。
#[derive(Debug, Clone, Default)]
pub struct EffectResult {
    /// 本次执行产生的世界关系 (source, predicate, target)。
    pub relations: Vec<(String, String, String)>,
}

/// 正向执行器。
pub trait EffectExecutor: Send + Sync {
    fn execute(&mut self, effect: &crate::kernel::Effect) -> Result<EffectResult, EffectError>;
}

/// V2-native 关系执行器：基于 RuntimeDefinition 的效应声明，
/// 用 Kernel relations 表达世界变化（State = Relation 投影）。
pub struct RelationEffectExecutor<'k> {
    kernel: &'k mut Kernel,
    /// 对象类型 → 效应声明查找（effect.action 语义名 → CapabilityDefinition）。
    defs: Vec<crate::compose::defs::RuntimeDefinition>,
}

impl<'k> RelationEffectExecutor<'k> {
    pub fn new(kernel: &'k mut Kernel, defs: Vec<crate::compose::defs::RuntimeDefinition>) -> Self {
        Self { kernel, defs }
    }

    fn effects_of(&self, action: &str) -> Vec<crate::compose::defs::EffectDefinition> {
        self.defs
            .iter()
            .flat_map(|d| &d.capabilities)
            .find(|c| c.action == action)
            .map(|c| c.effects.clone())
            .unwrap_or_default()
    }
}

impl EffectExecutor for RelationEffectExecutor<'_> {
    fn execute(&mut self, effect: &crate::kernel::Effect) -> Result<EffectResult, EffectError> {
        let ctx = self
            .kernel
            .fiber(effect.fiber)
            .map(|f| f.context)
            .ok_or_else(|| EffectError::Failed(format!("fiber 不存在: {}", effect.fiber)))?;
        let target = effect
            .input
            .get("target")
            .and_then(|v| v.as_str())
            .ok_or_else(|| EffectError::Failed("Effect 缺 target 参数".into()))?
            .to_string();

        let defs = self.effects_of(&effect.action);
        if defs.is_empty() {
            return Err(EffectError::NoEffects(effect.action.clone()));
        }
        let mut result = EffectResult::default();
        for def in defs {
            match def {
                crate::compose::defs::EffectDefinition::Update { field, value } => {
                    // State = Relation 投影：target --field--> 求值后的 value
                    let value = match value {
                        crate::compose::defs::EffectValue::Literal(v) => v.clone(),
                        crate::compose::defs::EffectValue::Param(p) => {
                            effect.input.get(p.as_str()).cloned().unwrap_or(Value::Null)
                        }
                        crate::compose::defs::EffectValue::SelfRef => Value::String(target.clone()),
                        crate::compose::defs::EffectValue::SelfField(f) => {
                            Value::String(format!("self.{f}"))
                        }
                    };
                    let value_s = value
                        .as_str()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| value.to_string());
                    let v2 = value_s.clone();
                    self.kernel
                        .relate(Relation::new(ctx, &target, &field, v2, self.kernel.now()))
                        .map_err(|e: KernelError| EffectError::Failed(e.to_string()))?;
                    result.relations.push((target.clone(), field, value_s));
                }
                crate::compose::defs::EffectDefinition::Create {
                    object_type,
                    link_edge,
                } => {
                    // 创建实体：target --link_edge--> new_object（缺省 id = target_action）
                    let new_id = format!("{}_{}", target, effect.action.to_lowercase());
                    if let Some(edge) = &link_edge {
                        self.kernel
                            .relate(Relation::new(
                                ctx,
                                &target,
                                edge,
                                &new_id,
                                self.kernel.now(),
                            ))
                            .map_err(|e: KernelError| EffectError::Failed(e.to_string()))?;
                        result
                            .relations
                            .push((target.clone(), edge.clone(), new_id));
                    } else {
                        result.relations.push((
                            target.clone(),
                            format!("created_{object_type}"),
                            new_id,
                        ));
                    }
                }
            }
        }
        Ok(result)
    }
}
