//! Skill Export —— Skill Asset → SKILL.md（Canonical Internal Representation；
//! 第三方格式只是 Export Adapter）。

use super::asset::SkillAsset;

/// Asset → SKILL.md（roundtrip 可再导入）。
pub fn export_skill_md(asset: &SkillAsset) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", asset.name));
    out.push_str(&format!("- goal: {}\n\n", asset.goal));
    if !asset.inputs.is_empty() {
        for i in &asset.inputs {
            out.push_str(&format!("- input: {i}\n"));
        }
        out.push('\n');
    }
    if !asset.outputs.is_empty() {
        for o in &asset.outputs {
            out.push_str(&format!("- output: {o}\n"));
        }
        out.push('\n');
    }
    for (i, w) in asset.workflow.iter().enumerate() {
        out.push_str(&format!("## Stage {}: {}\n", i + 1, w.name));
        for c in &asset.capabilities {
            out.push_str(&format!("- capability: {c}\n"));
        }
        for a in &asset.artifacts {
            out.push_str(&format!("- artifact: {}\n", a.name));
        }
        out.push('\n');
    }
    for p in &asset.policies {
        out.push_str(&format!("- policy: {}\n", p.condition));
    }
    for m in &asset.metrics {
        out.push_str(&format!("- metric: {}\n", m.name));
    }
    for f in &asset.feedback_loops {
        out.push_str(&format!("- feedback: {} -> {}\n", f.from, f.to));
    }
    out
}
