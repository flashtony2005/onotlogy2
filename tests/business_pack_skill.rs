//! 业务包操作 Skill 的 Markdown 和宿主注册契约。

use openaip_os::skill::builtin::{
    BUSINESS_PACK_OPERATIONS_SKILL_ID, register_business_pack_operations,
};
use openaip_os::skill::parser::{AISemanticParser, MarkdownSkillParser};
use openaip_os::skill::raw::RawSkill;
use openaip_os::skill::registry::SkillRegistry;

const BUSINESS_PACK_SKILL: &str = include_str!("../skills/business-pack-operations/SKILL.md");

#[test]
fn business_pack_operations_skill_is_parseable() {
    let raw = RawSkill::import("business-pack-operations", BUSINESS_PACK_SKILL);
    let ir = MarkdownSkillParser
        .parse(&raw)
        .expect("业务包操作 Skill 应可解析");

    assert_eq!(ir.identity.name, "Business Pack Operations");
    assert!(ir.goal.value.contains("校验"));
    assert!(
        ir.workflow
            .iter()
            .any(|step| step.value.name == "Identify Pack")
    );
    assert!(
        ir.workflow
            .iter()
            .any(|step| step.value.name == "Execute Lottery Capabilities")
    );
    assert!(
        ir.outputs
            .iter()
            .any(|output| output.value.contains("安装报告"))
    );
    assert!(
        ir.capabilities
            .iter()
            .any(|cap| { cap.value.capability == "lottery.data.sync" })
    );
}

#[test]
fn business_pack_operations_skill_registers_through_standard_pipeline() {
    let mut registry = SkillRegistry::new();
    let asset = register_business_pack_operations(&mut registry).expect("注册业务包 Skill");

    assert_eq!(asset.id, BUSINESS_PACK_OPERATIONS_SKILL_ID);
    assert_eq!(
        asset.status,
        openaip_os::skill::asset::SkillStatus::Imported
    );
    assert_eq!(
        asset.capabilities,
        vec![
            "lottery.data.sync",
            "lottery.import",
            "lottery.analyze",
            "lottery.backtest",
            "lottery.review",
        ]
    );
    assert!(registry.get(BUSINESS_PACK_OPERATIONS_SKILL_ID).is_some());
    assert!(registry.raw_of(BUSINESS_PACK_OPERATIONS_SKILL_ID).is_some());

    let duplicate =
        register_business_pack_operations(&mut registry).expect_err("重复注册必须被拒绝");
    assert!(duplicate.to_string().contains("重复注册"));
}

#[test]
fn v2_state_bootstraps_business_pack_operations_skill() {
    let state = openaip_os::server::v2::V2State::with_builtins().expect("V2 状态应装配内置 Skill");
    let registry = state.skills.read().expect("Skill registry 读锁");
    let asset = registry
        .get(BUSINESS_PACK_OPERATIONS_SKILL_ID)
        .expect("启动后应存在业务包操作 Skill");

    assert_eq!(
        asset.status,
        openaip_os::skill::asset::SkillStatus::Imported
    );
    assert!(registry.raw_of(BUSINESS_PACK_OPERATIONS_SKILL_ID).is_some());
}
