//! 完整 Action Engine（对应 REQ-014 / V0.4）
//!
//! V0.3 的"最小 Action 执行"是 Planner 内硬编码的 `build_retention_plan` + `Capability` 枚举；
//! V0.4 把它升级为**声明式、通用的 Action Engine**：动作写在对象上（ODL `actions:` → IR
//! `ActionDef`），由本模块解释执行（updates / creates / link），不再为每类动作写专属分支。
//!
//! 设计要点：
//! - `ActionRegistry`：从 IR 构建 `类型 → 动作名 → ActionDef` 映射，运行时驱动。
//! - `ActionEngine::invoke`：纯解释执行，幂等（upsert + 边去重），并产出 world 事件
//!   （`action.<name>`）把动作纳入世界模型生命周期，供三层观察 UI 回放。

use crate::ir::{ActionDef, OntologyIR, ValueExpr};
use crate::runtime::RuntimeKernel;
use crate::storage::StorageError;
use serde_json::{Value, json};
use std::collections::HashMap;

/// 动作注册表：类型 → 动作名 → 定义。
#[derive(Debug, Clone, Default)]
pub struct ActionRegistry {
    map: HashMap<String, HashMap<String, ActionDef>>,
}

impl ActionRegistry {
    /// 空注册表（无 ODL 时，Planner 走 legacy 兜底）。
    pub fn empty() -> Self {
        Self::default()
    }

    /// 从 IR 构建注册表：每个对象类型的 `actions` 登记进来。
    pub fn from_ir(ir: &OntologyIR) -> Self {
        let mut map: HashMap<String, HashMap<String, ActionDef>> = HashMap::new();
        for ot in &ir.object_types {
            let mut actions = HashMap::new();
            for a in &ot.actions {
                actions.insert(a.name.clone(), a.clone());
            }
            map.insert(ot.name.clone(), actions);
        }
        Self { map }
    }

    /// 取某类型某动作的注册定义。
    pub fn get(&self, object_type: &str, action: &str) -> Option<&ActionDef> {
        self.map.get(object_type).and_then(|m| m.get(action))
    }

    /// 列出某类型声明的全部动作。
    pub fn actions_of(&self, object_type: &str) -> Vec<&ActionDef> {
        self.map
            .get(object_type)
            .map(|m| m.values().collect())
            .unwrap_or_default()
    }

    /// 某类型是否声明了动作。
    pub fn has_actions(&self, object_type: &str) -> bool {
        self.map.get(object_type).is_some_and(|m| !m.is_empty())
    }

    /// 注册表是否为空（无 ODL 时为真，Planner 走 legacy 兜底）。
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// Action 执行错误，映射到 WB 错误码（落库失败透传 StorageError）。
#[derive(Debug, thiserror::Error)]
pub enum ActionError {
    #[error(transparent)]
    Storage(#[from] StorageError),
    /// 目标对象不存在（动作必须有受体）。
    #[error("动作目标对象不存在: {0}")]
    TargetNotFound(String),
    /// 引用了动作未声明的参数。
    #[error("动作引用未知参数 '{0}'")]
    UnknownParam(String),
}

/// 一次动作执行的效应摘要（供 Planner / 观察层使用）。
#[derive(Debug, Clone, Default)]
pub struct ActionEffect {
    /// 被更新的对象 ID。
    pub updated: Vec<String>,
    /// 被创建的对象 ID。
    pub created: Vec<String>,
    /// 建立的关系边 `(source, edge, target)`。
    pub linked: Vec<(String, String, String)>,
}

/// 通用动作执行引擎：解释 `ActionDef` 的 effects 并作用于世界模型。
pub struct ActionEngine<'a> {
    kernel: &'a RuntimeKernel,
}

impl<'a> ActionEngine<'a> {
    pub fn new(kernel: &'a RuntimeKernel) -> Self {
        Self { kernel }
    }

    /// 在 `target_id` 上执行动作 `def`，参数来自 `args`（动作参数名 → 值）。
    /// 幂等：更新走 upsert，关系边去重；执行后产出 `action.<name>` world 事件。
    pub async fn invoke(
        &self,
        def: &ActionDef,
        target_id: &str,
        args: &HashMap<String, Value>,
    ) -> Result<ActionEffect, ActionError> {
        let mut effect = ActionEffect::default();

        // 目标对象必须存在（动作是"对象的未来行为"，受体不可缺）
        let target = self
            .kernel
            .get_object(target_id)
            .await?
            .ok_or_else(|| ActionError::TargetNotFound(target_id.to_string()))?;
        let target_data = target.data.clone();

        // 1) updates：改写目标对象字段（每次重读，避免多字段互相覆盖）
        for u in &def.updates {
            let val = resolve(&u.value, args, &target_data, target_id);
            let mut data = self
                .kernel
                .get_object(target_id)
                .await?
                .map(|r| r.data.clone())
                .unwrap_or_else(|| json!({}));
            if let Some(obj) = data.as_object_mut() {
                obj.insert(u.field.clone(), val);
            }
            self.kernel.update_object(target_id, data).await?;
            effect.updated.push(target_id.to_string());
        }

        // 2) creates：新建对象（可选经 link_edge 与目标连边）
        for c in &def.creates {
            let mut data = json!({});
            if let Some(obj) = data.as_object_mut() {
                for (k, ve) in &c.data {
                    obj.insert(k.clone(), resolve(ve, args, &target_data, target_id));
                }
            }
            let new_id = match &c.id {
                Some(id) => id.clone(),
                None => format!("{target_id}_{}", def.name),
            };
            // 幂等 upsert：已存在则更新，避免二次运行主键冲突
            if self.kernel.get_object(&new_id).await?.is_some() {
                self.kernel.update_object(&new_id, data.clone()).await?;
            } else {
                self.kernel
                    .create_object(&c.object_type, &new_id, data.clone())
                    .await?;
            }
            effect.created.push(new_id.clone());

            if let Some(edge) = &c.link_edge {
                let exists = self
                    .kernel
                    .neighbors(target_id)
                    .await?
                    .iter()
                    .any(|r| r.relation == *edge && r.target == new_id);
                if !exists {
                    self.kernel.link_relation(target_id, edge, &new_id).await?;
                }
                effect
                    .linked
                    .push((target_id.to_string(), edge.clone(), new_id.clone()));
            }
        }

        // 3) 记录动作执行 world 事件（动作进入世界模型生命周期，可观测）
        let mut refs = vec![target_id.to_string()];
        refs.extend(effect.created.iter().cloned());
        let ref_strs: Vec<&str> = refs.iter().map(|s| s.as_str()).collect();
        self.kernel
            .emit_domain_event(&format!("action.{}", def.name), &ref_strs)
            .await?;

        Ok(effect)
    }
}

/// 解析值表达式：字面量 / 参数 / 自身 ID / 自身字段。
fn resolve(v: &ValueExpr, args: &HashMap<String, Value>, target: &Value, target_id: &str) -> Value {
    match v {
        ValueExpr::Literal(l) => l.clone(),
        ValueExpr::Param(p) => args.get(p).cloned().unwrap_or(Value::Null),
        ValueExpr::SelfId => Value::String(target_id.to_string()),
        ValueExpr::SelfField(f) => target.get(f).cloned().unwrap_or(Value::Null),
    }
}
