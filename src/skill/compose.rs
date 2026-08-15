//! Skill Composition —— A + B → New Candidate（重新验证后才可注册；原资产不动）。

use super::asset::SkillAsset;
use super::ir::SkillType;
use super::lineage::Lineage;

/// 组合：workflow 拼接、capabilities/artifacts/metrics 并集、反馈环合并。
/// 产物是 **Candidate**（version 0.1，status Imported）——必须重新 Validation 才可注册。
pub fn compose(a: &SkillAsset, b: &SkillAsset, name: &str) -> SkillAsset {
    let mut workflow = a.workflow.clone();
    for w in &b.workflow {
        if !workflow.iter().any(|x| x.name == w.name) {
            workflow.push(w.clone());
        }
    }
    let mut capabilities = a.capabilities.clone();
    for c in &b.capabilities {
        if !capabilities.contains(c) {
            capabilities.push(c.clone());
        }
    }
    let mut artifacts = a.artifacts.clone();
    for x in &b.artifacts {
        if !artifacts.iter().any(|y| y.name == x.name) {
            artifacts.push(x.clone());
        }
    }
    let mut metrics = a.metrics.clone();
    for m in &b.metrics {
        if !metrics.iter().any(|y| y.name == m.name) {
            metrics.push(m.clone());
        }
    }
    let mut feedback_loops = a.feedback_loops.clone();
    for f in &b.feedback_loops {
        if !feedback_loops.contains(f) {
            feedback_loops.push(f.clone());
        }
    }
    SkillAsset {
        id: name.to_string(),
        name: name.to_string(),
        version: "0.1".into(),
        skill_type: SkillType::Composite,
        goal: name.to_string(),
        inputs: a.inputs.clone(),
        outputs: a.outputs.clone(),
        workflow,
        capabilities,
        artifacts,
        preconditions: a.preconditions.clone(),
        postconditions: a.postconditions.clone(),
        policies: a.policies.clone(),
        metrics,
        feedback_loops,
        provenance: a.provenance.clone(),
        status: super::asset::SkillStatus::Imported,
        lineage: Lineage {
            parent: Some(a.id.clone()),
            derived_from: Some(format!("{}@{}", a.id, a.version)),
            ..Lineage::new("0.1")
        },
    }
}
