//! SkillRegistry —— 能力资产库（"如何完成目标"）。

use std::collections::HashMap;

use super::asset::{SkillAsset, SkillStatus};
use super::raw::RawSkill;

/// 注册表错误。
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("重复注册: {0}")]
    Duplicate(String),
    #[error("未找到 Skill: {0}")]
    NotFound(String),
    #[error("非法生命周期转移: {from} → {to}")]
    InvalidTransition { from: String, to: String },
}

/// 生命周期允许边（MVP 链式 + 终态）。
fn transition_allowed(from: SkillStatus, to: SkillStatus) -> bool {
    if from == to {
        return true; // 幂等
    }
    matches!(
        (from, to),
        (SkillStatus::Imported, SkillStatus::Parsed)
            | (SkillStatus::Parsed, SkillStatus::Normalized)
            | (SkillStatus::Normalized, SkillStatus::Resolved)
            | (SkillStatus::Resolved, SkillStatus::Validated)
            | (SkillStatus::Validated, SkillStatus::Registered)
            | (SkillStatus::Registered, SkillStatus::Active)
            | (SkillStatus::Active, SkillStatus::Forked)
            | (SkillStatus::Forked, SkillStatus::Experimental)
            | (SkillStatus::Experimental, SkillStatus::Evaluated)
            | (SkillStatus::Evaluated, SkillStatus::Promoted)
            | (SkillStatus::Active, SkillStatus::Deprecated)
            | (SkillStatus::Registered, SkillStatus::Deprecated)
            | (SkillStatus::Deprecated, SkillStatus::Archived)
    )
}

/// Skill Registry。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SkillRegistry {
    assets: HashMap<String, SkillAsset>,
    /// 原始快照（P1 Raw 不修改：与编译资产并存，可追溯）。
    raws: HashMap<String, RawSkill>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册（重复 id → Err；幂等保护）。
    pub fn register(&mut self, asset: SkillAsset) -> Result<(), RegistryError> {
        if self.assets.contains_key(&asset.id) {
            return Err(RegistryError::Duplicate(asset.id.clone()));
        }
        self.assets.insert(asset.id.clone(), asset);
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&SkillAsset> {
        self.assets.get(id)
    }

    /// 关联原始快照（导入时调用；重复 attach 以首次为准——Raw 不可变）。
    pub fn attach_raw(&mut self, id: &str, raw: RawSkill) {
        self.raws.entry(id.to_string()).or_insert(raw);
    }

    pub fn raw_of(&self, id: &str) -> Option<&RawSkill> {
        self.raws.get(id)
    }

    pub fn all(&self) -> Vec<&SkillAsset> {
        self.assets.values().collect()
    }

    /// 生命周期推进（非法跳转 → Err）。
    pub fn advance_status(&mut self, id: &str, to: SkillStatus) -> Result<(), RegistryError> {
        let asset = self
            .assets
            .get_mut(id)
            .ok_or_else(|| RegistryError::NotFound(id.to_string()))?;
        let from = asset.status;
        if !transition_allowed(from, to) {
            return Err(RegistryError::InvalidTransition {
                from: status_name(from),
                to: status_name(to),
            });
        }
        asset.status = to;
        Ok(())
    }

    /// 废弃（ACTIVE/REGISTERED → DEPRECATED）。
    pub fn deprecate(&mut self, id: &str) -> Result<(), RegistryError> {
        self.advance_status(id, SkillStatus::Deprecated)
    }

    /// 归档（DEPRECATED → ARCHIVED）。
    pub fn archive(&mut self, id: &str) -> Result<(), RegistryError> {
        self.advance_status(id, SkillStatus::Archived)
    }

    /// 搜索（MVP：name / goal / capability 子串匹配）。
    pub fn search(&self, q: &str) -> Vec<&SkillAsset> {
        let q = q.to_lowercase();
        self.assets
            .values()
            .filter(|a| {
                a.name.to_lowercase().contains(&q)
                    || a.goal.to_lowercase().contains(&q)
                    || a.capabilities.iter().any(|c| c.to_lowercase().contains(&q))
            })
            .collect()
    }
}

fn status_name(s: SkillStatus) -> String {
    format!("{s:?}")
}
