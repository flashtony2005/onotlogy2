//! AISemanticParser —— AI 语义解析（P2：AI 负责理解，不负责授权）。
//!
//! MVP：确定性 `MarkdownSkillParser`（标题层级/列表 → 结构化提取，span = 行号区间，
//! 置信度 1.0）——接口就位，未来接 Pi/LLM（无 LLM 环境，与 FakeEngine 同模式）。

use super::ir::{
    ArtifactRef, CapabilityRef, FeedbackLoop, Inferred, MetricIR, ParseError, PolicyIR, Provenance,
    RelationIR, SkillIR, SkillIdentity, SkillType, SourceSpan, WorkflowStep,
};
use super::raw::RawSkill;

/// AI 语义解析器接口（可替换：Markdown / LLM / MCP 解析器）。
pub trait AISemanticParser {
    fn parse(&self, raw: &RawSkill) -> Result<SkillIR, ParseError>;
}

/// 确定性 Markdown 解析器。
///
/// 规则（按行解析，行号为 span）：
/// - `# Title` → identity.name
/// - `- goal: X` → goal
/// - `## Stage N: Name` → workflow step（顺序）
/// - `### Sub-skill: Name` → sub_skills
/// - `- capability: X` → capabilities
/// - `- artifact: X` → artifacts
/// - `- metric: X` → metrics
/// - `- feedback: A -> B` → feedback_loops
/// - `- input: X` / `- output: X` → inputs / outputs
/// - `- policy: ...` → PolicyIR（含 approval/审批 → requirement 标注）
/// - `- relation: A predicate B` → relations（Ontology Discovery）
pub struct MarkdownSkillParser;

impl AISemanticParser for MarkdownSkillParser {
    fn parse(&self, raw: &RawSkill) -> Result<SkillIR, ParseError> {
        let mut identity = SkillIdentity {
            name: raw.name.clone(),
            description: String::new(),
        };
        let mut goal: Option<Inferred<String>> = None;
        let mut inputs: Vec<Inferred<String>> = Vec::new();
        let mut outputs: Vec<Inferred<String>> = Vec::new();
        let mut workflow: Vec<Inferred<WorkflowStep>> = Vec::new();
        let mut sub_skills: Vec<Inferred<String>> = Vec::new();
        let mut capabilities: Vec<Inferred<CapabilityRef>> = Vec::new();
        let mut artifacts: Vec<Inferred<ArtifactRef>> = Vec::new();
        let mut preconditions: Vec<Inferred<String>> = Vec::new();
        let mut postconditions: Vec<Inferred<String>> = Vec::new();
        let mut policies: Vec<Inferred<PolicyIR>> = Vec::new();
        let mut metrics: Vec<Inferred<MetricIR>> = Vec::new();
        let mut feedback_loops: Vec<Inferred<FeedbackLoop>> = Vec::new();
        let mut constraints: Vec<Inferred<String>> = Vec::new();
        let mut relations: Vec<Inferred<RelationIR>> = Vec::new();

        let mut current_stage: Option<String> = None;

        for (idx, line) in raw.content.lines().enumerate() {
            let lineno = idx + 1;
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            // 标题
            if let Some(title) = line.strip_prefix("# ") {
                identity.name = title.trim().to_string();
                continue;
            }
            if line.starts_with("## ") {
                let name = line.trim_start_matches("## ").trim();
                // "Stage 1: Niche Research" → 阶段名取冒号后（无冒号取整段）
                let stage_name = name.split_once(':').map(|(_, n)| n.trim()).unwrap_or(name);
                current_stage = Some(stage_name.to_string());
                workflow.push(Inferred::new(
                    WorkflowStep {
                        name: stage_name.to_string(),
                        description: String::new(),
                    },
                    SourceSpan::at(lineno),
                ));
                continue;
            }
            if let Some(name) = line.strip_prefix("### Sub-skill: ") {
                sub_skills.push(Inferred::new(
                    name.trim().to_string(),
                    SourceSpan::at(lineno),
                ));
                continue;
            }
            // 键值列表
            if let Some(v) = line.strip_prefix("- goal: ") {
                goal = Some(Inferred::new(v.trim().to_string(), SourceSpan::at(lineno)));
                continue;
            }
            if let Some(v) = line.strip_prefix("- input: ") {
                inputs.push(Inferred::new(v.trim().to_string(), SourceSpan::at(lineno)));
                continue;
            }
            if let Some(v) = line.strip_prefix("- output: ") {
                outputs.push(Inferred::new(v.trim().to_string(), SourceSpan::at(lineno)));
                continue;
            }
            if let Some(v) = line.strip_prefix("- capability: ") {
                capabilities.push(Inferred::new(
                    CapabilityRef {
                        capability: v.trim().to_string(),
                    },
                    SourceSpan::at(lineno),
                ));
                continue;
            }
            if let Some(v) = line.strip_prefix("- artifact: ") {
                artifacts.push(Inferred::new(
                    ArtifactRef {
                        name: v.trim().to_string(),
                        artifact_type: current_stage.clone().unwrap_or_else(|| "Artifact".into()),
                    },
                    SourceSpan::at(lineno),
                ));
                continue;
            }
            if let Some(v) = line.strip_prefix("- metric: ") {
                metrics.push(Inferred::new(
                    MetricIR {
                        name: v.trim().to_string(),
                        domain: None,
                    },
                    SourceSpan::at(lineno),
                ));
                continue;
            }
            if let Some(v) = line.strip_prefix("- feedback: ") {
                if let Some((from, to)) = v.split_once("->").or_else(|| v.split_once('→')) {
                    feedback_loops.push(Inferred::new(
                        FeedbackLoop {
                            from: from.trim().to_string(),
                            to: to.trim().to_string(),
                        },
                        SourceSpan::at(lineno),
                    ));
                }
                continue;
            }
            if let Some(v) = line.strip_prefix("- policy: ") {
                let v = v.trim();
                let (requirement, condition) = if v.contains("审批") || v.contains("批准") {
                    // "发布超过 500 美元广告需要管理员批准" → requirement 标注审批
                    let cond = v.split("需要").next().unwrap_or(v).trim().to_string();
                    ("approval.manager".to_string(), cond)
                } else {
                    ("none".to_string(), v.to_string())
                };
                policies.push(Inferred::new(
                    PolicyIR {
                        condition,
                        requirement,
                    },
                    SourceSpan::at(lineno),
                ));
                continue;
            }
            if let Some(v) = line.strip_prefix("- relation: ") {
                // "Storyboard contains Scene" → source/predicate/target（三分）
                let parts: Vec<&str> = v.split_whitespace().collect();
                if parts.len() == 3 {
                    relations.push(Inferred::new(
                        RelationIR {
                            source: parts[0].to_string(),
                            predicate: parts[1].to_string(),
                            target: parts[2].to_string(),
                        },
                        SourceSpan::at(lineno),
                    ));
                }
                continue;
            }
            if let Some(v) = line.strip_prefix("- precondition: ") {
                preconditions.push(Inferred::new(v.trim().to_string(), SourceSpan::at(lineno)));
                continue;
            }
            if let Some(v) = line.strip_prefix("- postcondition: ") {
                postconditions.push(Inferred::new(v.trim().to_string(), SourceSpan::at(lineno)));
                continue;
            }
            // 其他列表项 → 约束（尽力）
            if let Some(v) = line.strip_prefix("- ") {
                constraints.push(Inferred::new(v.trim().to_string(), SourceSpan::at(lineno)));
            }
        }

        // 描述：goal 前的非标题段落（MVP：第一段非空正文）
        for line in raw.content.lines() {
            let t = line.trim();
            if t.starts_with('#') || t.starts_with('-') || t.is_empty() {
                continue;
            }
            identity.description = t.to_string();
            break;
        }

        // 类型判定：子技能 ≥ 2 → Composite；阶段 ≥ 2 → Workflow；否则 Atomic
        let skill_type = if sub_skills.len() >= 2 {
            SkillType::Composite
        } else if workflow.len() >= 2 {
            SkillType::Workflow
        } else {
            SkillType::Atomic
        };

        Ok(SkillIR {
            identity,
            skill_type,
            goal: goal.unwrap_or_else(|| Inferred::new(String::new(), SourceSpan::at(1))),
            inputs,
            outputs,
            workflow,
            sub_skills,
            capabilities,
            artifacts,
            preconditions,
            postconditions,
            policies,
            metrics,
            feedback_loops,
            constraints,
            relations,
            provenance: Provenance {
                raw_skill_id: raw.id.clone(),
                parser: "MarkdownSkillParser".into(),
            },
        })
    }
}
