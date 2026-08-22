//! 宿主内置 Skill 资产。
//!
//! 业务包操作 Skill 以 Markdown 作为可追溯原始快照，启动时经过
//! Raw → IR → Capability Resolution → Asset → Registry 的标准链路注册。

use super::asset::{AssetCompiler, AssetError, SkillAsset, Validator};
use super::parser::{AISemanticParser, MarkdownSkillParser};
use super::raw::RawSkill;
use super::registry::{RegistryError, SkillRegistry};
use super::resolver::SkillResolver;
use thiserror::Error;

pub const BUSINESS_PACK_OPERATIONS_SKILL_ID: &str = "business-pack-operations";

const BUSINESS_PACK_OPERATIONS_SKILL_MD: &str =
    include_str!("../../skills/business-pack-operations/SKILL.md");

const BUSINESS_PACK_CAPABILITIES: &[&str] = &[
    "lottery.data.sync",
    "lottery.import",
    "lottery.analyze",
    "lottery.backtest",
    "lottery.review",
];

#[derive(Debug, Error)]
pub enum BuiltinSkillError {
    #[error("Skill 解析失败：{0}")]
    Parse(#[from] super::ir::ParseError),
    #[error("Skill 编译失败：{0}")]
    Compile(#[from] AssetError),
    #[error("Skill 校验失败：{errors:?}")]
    Validation { errors: Vec<String> },
    #[error("Skill 注册失败：{0}")]
    Registry(#[from] RegistryError),
}

/// 将业务包操作 Skill 按标准编译链路注册到 SkillRegistry。
///
/// 该函数只注册 Skill 资产，不注册插件能力。能力的真正执行仍由
/// PluginCatalog 负责，避免 Skill 解析过程绕过宿主授权边界。
pub fn register_business_pack_operations(
    registry: &mut SkillRegistry,
) -> Result<SkillAsset, BuiltinSkillError> {
    let raw = RawSkill::import(
        BUSINESS_PACK_OPERATIONS_SKILL_ID,
        BUSINESS_PACK_OPERATIONS_SKILL_MD,
    );
    let ir = MarkdownSkillParser.parse(&raw)?;
    let resolutions = SkillResolver::new(BUSINESS_PACK_CAPABILITIES).resolve_capabilities(&ir);
    let asset = AssetCompiler::default().compile(&raw, &ir, &resolutions)?;
    let report = Validator::new(BUSINESS_PACK_CAPABILITIES).validate(&asset);
    if !report.ok {
        return Err(BuiltinSkillError::Validation {
            errors: report.errors,
        });
    }
    registry.register(asset.clone())?;
    let raw_id = raw.id.clone();
    registry.attach_raw(&raw_id, raw);
    Ok(asset)
}
