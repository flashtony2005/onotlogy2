//! Skill Mutation Apply（Phase 11B 诊断环的「Apply」落点）。
//!
//! Mutation 是 Candidate Asset（Skill 永不原地修改）：评审通过后，把结构化 Delta
//! 应用到 SkillAsset 的工作流（Fork），注册为 EXPERIMENTAL 候选——原资产保留，
//! 晋升 ACTIVE 仍需后续 Benchmark + Human/闸门（Learn ≠ Permission）。

use serde_json::Value;

use crate::mutation::{MutationError, validate_delta};
use crate::skill::asset::{SkillAsset, SkillStatus};
use crate::skill::ir::WorkflowStep;
use crate::skill::lineage::Lineage;
use crate::skill::registry::{RegistryError, SkillRegistry};

/// 把结构化 Delta 应用到 SkillAsset 的工作流（纯函数；原资产不动，返回新资产）。
///
/// 操作（白名单，与 mutation::validate_delta 同源）：
/// - `insert_step`：`{ step, after? }` —— after 缺省 = 追加到末尾
/// - `modify`：`{ step, description? }` —— 更新步骤描述（人工细化草稿的载体）
/// - `delete_step`：`{ step }` —— 删除步骤
/// - `replace`：`{ step, with }` —— 替换步骤名
pub fn apply_delta(asset: &SkillAsset, delta: &Value) -> Result<SkillAsset, MutationError> {
    validate_delta(delta)?;
    let obj = delta.as_object().expect("validate_delta 已保证对象");
    let mut workflow = asset.workflow.clone();
    for (op, spec) in obj {
        let spec_obj = spec.as_object().expect("validate_delta 已保证对象");
        match op.as_str() {
            "insert_step" => {
                let step_name = spec_obj["step"].as_str().expect("已校验");
                let after = spec_obj.get("after").and_then(|v| v.as_str());
                let new_step = WorkflowStep {
                    name: step_name.to_string(),
                    description: format!("(mutation) {step_name}"),
                };
                match after {
                    Some(after_name) => {
                        let pos = workflow
                            .iter()
                            .position(|s| s.name == after_name)
                            .ok_or_else(|| {
                                MutationError::MissingField(format!(
                                    "insert_step.after: 步骤 {after_name} 不存在"
                                ))
                            })?;
                        workflow.insert(pos + 1, new_step);
                    }
                    None => workflow.push(new_step),
                }
            }
            "modify" => {
                let step_name = spec_obj["step"].as_str().expect("已校验");
                let pos = workflow
                    .iter()
                    .position(|s| s.name == step_name)
                    .ok_or_else(|| {
                        MutationError::MissingField(format!("modify.step: 步骤 {step_name} 不存在"))
                    })?;
                if let Some(desc) = spec_obj.get("description").and_then(|v| v.as_str()) {
                    workflow[pos].description = desc.to_string();
                }
            }
            "delete_step" => {
                let step_name = spec_obj["step"].as_str().expect("已校验");
                let pos = workflow
                    .iter()
                    .position(|s| s.name == step_name)
                    .ok_or_else(|| {
                        MutationError::MissingField(format!(
                            "delete_step.step: 步骤 {step_name} 不存在"
                        ))
                    })?;
                workflow.remove(pos);
            }
            "replace" => {
                let step_name = spec_obj["step"].as_str().expect("已校验");
                let with = spec_obj["with"].as_str().expect("已校验");
                let pos = workflow
                    .iter()
                    .position(|s| s.name == step_name)
                    .ok_or_else(|| {
                        MutationError::MissingField(format!(
                            "replace.step: 步骤 {step_name} 不存在"
                        ))
                    })?;
                workflow[pos].name = with.to_string();
            }
            _ => unreachable!("白名单已过滤"),
        }
    }
    let mut out = asset.clone();
    out.workflow = workflow;
    Ok(out)
}

/// Fork 候选：base 资产 + Delta → 新候选资产（EXPERIMENTAL，原资产保留），注册进 Registry。
///
/// 候选 id = `<base_id>@<candidate_version>`；lineage.parent/derived_from = base。
pub fn fork_candidate(
    registry: &mut SkillRegistry,
    base_id: &str,
    candidate_version: &str,
    delta: &Value,
) -> Result<SkillAsset, ApplyError> {
    let base = registry
        .get(base_id)
        .ok_or_else(|| ApplyError::BaseNotFound(base_id.to_string()))?
        .clone();
    let applied = apply_delta(&base, delta).map_err(ApplyError::Delta)?;
    let candidate_id = format!("{base_id}@{candidate_version}");
    let candidate = SkillAsset {
        id: candidate_id.clone(),
        name: base.name.clone(),
        version: candidate_version.to_string(),
        skill_type: base.skill_type,
        goal: base.goal,
        inputs: base.inputs,
        outputs: base.outputs,
        workflow: applied.workflow,
        capabilities: base.capabilities,
        artifacts: base.artifacts,
        preconditions: base.preconditions,
        postconditions: base.postconditions,
        policies: base.policies,
        metrics: base.metrics,
        feedback_loops: base.feedback_loops,
        provenance: base.provenance,
        status: SkillStatus::Experimental,
        lineage: Lineage {
            parent: Some(base_id.to_string()),
            derived_from: Some(base_id.to_string()),
            version: candidate_version.to_string(),
            author: base.lineage.author,
            license: base.lineage.license,
            origin: base.lineage.origin,
        },
    };
    registry
        .register(candidate.clone())
        .map_err(ApplyError::Registry)?;
    Ok(candidate)
}

/// Apply 错误。
#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error("基准 Skill 不存在：{0}")]
    BaseNotFound(String),
    #[error("Delta 应用失败：{0}")]
    Delta(MutationError),
    #[error("注册失败：{0}")]
    Registry(RegistryError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill::asset::SkillStatus;
    use crate::skill::lineage::Lineage;
    use crate::skill::registry::SkillRegistry;
    use serde_json::json;

    fn base_asset() -> SkillAsset {
        SkillAsset {
            id: "fix-java-bug".into(),
            name: "fix-java-bug".into(),
            version: "v1".into(),
            skill_type: crate::skill::ir::SkillType::Workflow,
            goal: "fix bug".into(),
            inputs: vec![],
            outputs: vec![],
            workflow: vec![
                WorkflowStep {
                    name: "modify_code".into(),
                    description: "修改代码".into(),
                },
                WorkflowStep {
                    name: "unit_test".into(),
                    description: "单元测试".into(),
                },
            ],
            capabilities: vec!["code.write".into()],
            artifacts: vec![],
            preconditions: vec![],
            postconditions: vec![],
            policies: vec![],
            metrics: vec![],
            feedback_loops: vec![],
            provenance: crate::skill::ir::Provenance {
                raw_skill_id: "raw-1".into(),
                parser: "test".into(),
            },
            status: SkillStatus::Active,
            lineage: Lineage::new("v1"),
        }
    }

    #[test]
    fn apply_insert_append_and_after() {
        let a = base_asset();
        // 追加（after 缺省）
        let a1 = apply_delta(&a, &json!({ "insert_step": { "step": "run_tests" } })).unwrap();
        assert_eq!(a1.workflow.len(), 3);
        assert_eq!(a1.workflow[2].name, "run_tests");
        // 指定 after
        let a2 = apply_delta(
            &a,
            &json!({ "insert_step": { "after": "modify_code", "step": "integration_test" } }),
        )
        .unwrap();
        assert_eq!(a2.workflow[1].name, "integration_test");
        // 原资产不动（Fork 语义）
        assert_eq!(a.workflow.len(), 2);
        // after 不存在 → 拒绝
        assert!(
            apply_delta(
                &a,
                &json!({ "insert_step": { "after": "nope", "step": "x" } })
            )
            .is_err()
        );
    }

    #[test]
    fn apply_modify_delete_replace() {
        let a = base_asset();
        let m = apply_delta(
            &a,
            &json!({ "modify": { "step": "unit_test", "description": "含回归检查" } }),
        )
        .unwrap();
        assert_eq!(m.workflow[1].description, "含回归检查");

        let d = apply_delta(&a, &json!({ "delete_step": { "step": "unit_test" } })).unwrap();
        assert_eq!(d.workflow.len(), 1);

        let r = apply_delta(
            &a,
            &json!({ "replace": { "step": "modify_code", "with": "edit" } }),
        )
        .unwrap();
        assert_eq!(r.workflow[0].name, "edit");

        // 自由文本拒绝
        assert!(apply_delta(&a, &json!("free text")).is_err());
    }

    #[test]
    fn fork_candidate_registers_with_lineage() {
        let mut reg = SkillRegistry::new();
        reg.register(base_asset()).unwrap();
        let cand = fork_candidate(
            &mut reg,
            "fix-java-bug",
            "v2",
            &json!({ "insert_step": { "after": "unit_test", "step": "integration_test" } }),
        )
        .unwrap();
        assert_eq!(cand.id, "fix-java-bug@v2");
        assert_eq!(
            cand.status,
            SkillStatus::Experimental,
            "候选只到 EXPERIMENTAL"
        );
        assert_eq!(cand.lineage.parent.as_deref(), Some("fix-java-bug"));
        assert_eq!(cand.workflow[2].name, "integration_test");
        // 原资产保留且未被修改
        assert_eq!(reg.get("fix-java-bug").unwrap().workflow.len(), 2);
        // 基准不存在 → 错误
        assert!(
            fork_candidate(
                &mut reg,
                "nope",
                "v2",
                &json!({ "insert_step": { "step": "x" } })
            )
            .is_err()
        );
    }
}
