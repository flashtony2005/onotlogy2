//! Diagnosis —— Agent Evolution（Phase 11B · 诊断环）。
//!
//! 职责：把 Phase 11A 产出的评测结果（wb_evaluation）转换为**可人工评审的
//! Mutation Proposal**——Proposal ≠ Mutation（Learn ≠ Permission）。
//!
//! 数据流（只消费已产生的评测数据，不新增数据流）：
//! ```text
//! Evaluation → Failure Pattern Detection → Diagnosis → Mutation Proposal
//!   → Human Review → Apply Mutation（fork 候选，原资产保留）
//! ```
//!
//! 诊断规则全部**确定性**（仓库阈值模式：hits≥2 才成模式，不迷信小样本统计）：
//! - T1 测试证据缺失：执行过但 correctness 未知（无 TestResult / relation / judge）
//! - T2 失败模式重复：同一 worker.failed error 在多个 run 出现
//! - T3 可靠性偏低：reliability < 阈值且在多个 run 出现

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::evaluation::EvaluationResult;
use crate::mutation::{MutationProposal, MutationStatus};
use crate::storage::StorageError;

/// 诊断配置（阈值模式，对齐 wb_pattern hits≥2）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosisConfig {
    /// 模式最少支持 run 数（默认 2）。
    pub min_runs: usize,
    /// 可靠性阈值（低于视为低可靠性模式，默认 0.8）。
    pub reliability_threshold: f64,
}

impl Default for DiagnosisConfig {
    fn default() -> Self {
        Self {
            min_runs: 2,
            reliability_threshold: 0.8,
        }
    }
}

/// 一条诊断：模式 + 根因 + 证据 + Delta 草稿（草稿需人工评审）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnosis {
    /// 幂等键：`diag.<rule>.<key>`（同模式不重复提案）。
    pub id: String,
    /// 人类可读模式描述。
    pub pattern: String,
    /// 根因分析。
    pub root_cause: String,
    /// 确定性置信度（支持 run 数 / min_runs 截断）。
    pub confidence: f64,
    /// 证据 run 列表（可追溯至 wb_evaluation）。
    pub evidence_runs: Vec<String>,
    /// 结构化 Delta 草稿（白名单操作；人工评审可细化）。
    pub suggested_delta: Value,
}

/// 诊断器（纯函数；确定性）。
#[derive(Debug, Clone, Default)]
pub struct Diagnoser {
    pub config: DiagnosisConfig,
}

impl Diagnoser {
    pub fn new(config: DiagnosisConfig) -> Self {
        Self { config }
    }

    /// 对同一 Skill 的一组评测结果做失败模式诊断（确定性规则；输入顺序无关）。
    pub fn diagnose(&self, results: &[EvaluationResult], skill: &str) -> Vec<Diagnosis> {
        let mut out = Vec::new();
        let min = self.config.min_runs.max(1);

        // ── T1：测试证据缺失（执行过但 correctness 未知）────────────────
        let no_evidence: Vec<&EvaluationResult> = results
            .iter()
            .filter(|r| r.correctness.is_unknown() && !r.efficiency.is_unknown())
            .collect();
        if no_evidence.len() >= min {
            out.push(Diagnosis {
                id: format!("diag.{skill}.missing-test-evidence"),
                pattern: format!("任务执行未产出正确性证据（{} 个 run）", no_evidence.len()),
                root_cause: "执行路径没有产生 TestResult / relation / judge 证据——".to_string(),
                confidence: confidence(no_evidence.len(), min),
                evidence_runs: no_evidence.iter().map(|r| r.run_id.clone()).collect(),
                suggested_delta: json!({
                    "insert_step": { "step": "run_tests" }
                }),
            });
        }

        // ── T2：失败模式重复（同一 worker.failed error 在多个 run 出现）──
        let mut mode_runs: Vec<(String, Vec<String>)> = Vec::new();
        for r in results {
            for mode in &r.failure_modes {
                match mode_runs.iter_mut().find(|(m, _)| m == mode) {
                    Some((_, runs)) => runs.push(r.run_id.clone()),
                    None => mode_runs.push((mode.clone(), vec![r.run_id.clone()])),
                }
            }
        }
        for (mode, runs) in mode_runs {
            if runs.len() >= min {
                out.push(Diagnosis {
                    id: format!("diag.{skill}.failure-mode.{mode}"),
                    pattern: format!("失败模式重复出现（{} 个 run）：{mode}", runs.len()),
                    root_cause: format!("worker 执行反复失败于：{mode}"),
                    confidence: confidence(runs.len(), min),
                    evidence_runs: runs.clone(),
                    suggested_delta: json!({
                        "modify": {
                            "step": "execute",
                            "description": format!("执行后检查是否出现失败模式：{mode}（人工细化）")
                        }
                    }),
                });
            }
        }

        // ── T3：可靠性偏低（reliability < 阈值且多 run）─────────────────
        let low_rel: Vec<&EvaluationResult> = results
            .iter()
            .filter(|r| {
                r.reliability
                    .value
                    .is_some_and(|v| v < self.config.reliability_threshold)
            })
            .collect();
        if low_rel.len() >= min {
            out.push(Diagnosis {
                id: format!("diag.{skill}.low-reliability"),
                pattern: format!(
                    "可靠性低于阈值 {:.2}（{} 个 run）",
                    self.config.reliability_threshold,
                    low_rel.len()
                ),
                root_cause: "worker 失败/重试占比过高".to_string(),
                confidence: confidence(low_rel.len(), min),
                evidence_runs: low_rel.iter().map(|r| r.run_id.clone()).collect(),
                suggested_delta: json!({
                    "modify": {
                        "step": "execute",
                        "description": "执行失败时增加一次重试/降级（人工细化）"
                    }
                }),
            });
        }

        // 确定性排序（幂等：同输入永远同输出顺序）。
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }
}

fn confidence(runs: usize, min: usize) -> f64 {
    (runs as f64 / (min * 3) as f64).min(1.0)
}

/// 诊断 → Mutation Proposal（PROPOSED，等待人工评审）。
pub fn propose_from_diagnosis(
    d: &Diagnosis,
    target_skill: &str,
    source_version: &str,
) -> Result<MutationProposal, crate::mutation::MutationError> {
    MutationProposal::propose(
        target_skill,
        source_version,
        &format!("{source_version}+{}", sanitize(rule_short(&d.id))),
        &d.root_cause,
        d.evidence_runs.clone(),
        d.suggested_delta.clone(),
    )
}

/// 从 id 提取规则短名（`diag.<skill>.failure-mode.a b` → `failure-mode.a b`）。
fn rule_short(id: &str) -> &str {
    id.rsplit('.').next().unwrap_or(id)
}

/// 规则短名的 id 安全清洗（空格 → '-'，保证提案 id / URL 安全）。
fn sanitize(s: &str) -> String {
    s.replace(' ', "-")
}

/// 编排：读评测 → 诊断 → 提案落库。
///
/// 语义（B 诊断环）：
/// - 新提案状态 **PROPOSED**（等待人工评审——Proposal ≠ Mutation）
/// - 幂等：同诊断 id 已存在且已评审（VALIDATED/REJECTED/…）→ **跳过不覆盖**（否决决定持久）
/// - 同诊断 id 仍是 PROPOSED → upsert 刷新证据
pub async fn run_diagnosis(
    storage: &crate::storage::LibSQLStorage,
    skill: &str,
    source_version: &str,
    task_id: Option<&str>,
    config: &DiagnosisConfig,
) -> Result<Vec<MutationProposal>, StorageError> {
    let results = storage.list_evaluations(None, task_id, 500).await?;
    let diags = Diagnoser::new(config.clone()).diagnose(&results, skill);
    let existing: std::collections::HashSet<String> = storage
        .list_mutations(None)
        .await?
        .into_iter()
        .filter(|p| p.status != MutationStatus::Proposed)
        .map(|p| p.id)
        .collect();
    let mut proposals = Vec::new();
    for d in diags {
        if existing.contains(&proposal_id_for(skill, source_version, &d)) {
            continue; // 已评审：不覆盖（REJECTED 决定持久）
        }
        let p = propose_from_diagnosis(&d, skill, source_version).map_err(diag_err)?;
        storage.save_mutation(&p).await?;
        proposals.push(p);
    }
    Ok(proposals)
}

/// 诊断 → 提案的幂等 id（与 MutationProposal::propose 的 id 规则一致；供 Benchmark 定位目标模式）。
pub fn proposal_id_for(skill: &str, source_version: &str, d: &Diagnosis) -> String {
    format!(
        "mutation.{skill}.{source_version}+{}",
        sanitize(rule_short(&d.id))
    )
}

/// MutationError → StorageError（提案落库路径统一错误类型）。
fn diag_err(e: crate::mutation::MutationError) -> StorageError {
    StorageError::Constraint {
        code: "WB3001",
        message: format!("诊断提案失败：{e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evaluation::{Score, TaskDefinition};
    use crate::mutation::{MutationProposal, MutationStatus};
    use crate::session::event::{EventType, SessionEvent, SessionId};
    use serde_json::json;

    fn ev(t: EventType, data: Value, seq: u64) -> SessionEvent {
        SessionEvent {
            session_id: SessionId::new("s1"),
            seq,
            event_type: t,
            timestamp: 1000 + seq as i64,
            data,
        }
    }

    fn task() -> TaskDefinition {
        TaskDefinition {
            task_id: "t1".into(),
            expected_relations: vec![],
            expected_artifacts: vec![],
            time_budget_secs: 10.0,
        }
    }

    /// 构造评测结果：可指定 correctness/reliability/failure_modes。
    fn result(
        run_id: &str,
        correctness: Option<f64>,
        reliability: Option<f64>,
        modes: &[&str],
    ) -> EvaluationResult {
        let reducer = crate::evaluation::EvaluationReducer::default();
        let mut events = vec![
            ev(
                EventType::WorkerStarted,
                json!({ "worker": "w1", "attempt": 1 }),
                0,
            ),
            ev(
                EventType::WorkerCompleted,
                json!({ "worker": "w1", "ok": true, "duration_ms": 5000.0 }),
                1,
            ),
        ];
        if let Some(c) = correctness {
            events.push(ev(
                EventType::TestResult,
                json!({ "tests_total": 1, "tests_passed": (c * 1.0) as u64 }),
                2,
            ));
        }
        for (i, m) in modes.iter().enumerate() {
            events.push(ev(
                EventType::WorkerFailed,
                json!({ "worker": "w1", "error": m }),
                10 + i as u64,
            ));
        }
        let mut r = reducer.evaluate(&events, run_id, &task());
        // 覆盖 reliability（失败事件已计入；此处允许显式指定）
        r.reliability = match reliability {
            Some(v) => Score::known(v, vec![]),
            None => r.reliability,
        };
        r
    }

    #[test]
    fn t1_missing_test_evidence_rule() {
        let config = DiagnosisConfig {
            min_runs: 2,
            ..Default::default()
        };
        let results = vec![
            result("r1", None, Some(1.0), &[]),
            result("r2", None, Some(1.0), &[]),
            result("r3", Some(0.9), Some(1.0), &[]), // 有证据，不参与
        ];
        let diags = Diagnoser::new(config).diagnose(&results, "fix-java-bug");
        let t1 = diags
            .iter()
            .find(|d| d.id.contains("missing-test-evidence"))
            .expect("T1 命中");
        assert_eq!(t1.evidence_runs, vec!["r1", "r2"]);
        assert_eq!(
            t1.suggested_delta["insert_step"]["step"],
            json!("run_tests")
        );
        assert!(t1.confidence > 0.0);
    }

    #[test]
    fn t2_repeated_failure_mode_rule() {
        let config = DiagnosisConfig {
            min_runs: 2,
            ..Default::default()
        };
        let results = vec![
            result("r1", Some(0.8), Some(1.0), &["integration timeout"]),
            result("r2", Some(0.8), Some(1.0), &["integration timeout"]),
            result("r3", Some(0.8), Some(1.0), &["other"]),
        ];
        let diags = Diagnoser::new(config).diagnose(&results, "fix-java-bug");
        let t2 = diags
            .iter()
            .find(|d| d.id.contains("failure-mode.integration timeout"))
            .expect("T2 命中");
        assert_eq!(t2.evidence_runs.len(), 2);
        assert!(
            !diags.iter().any(|d| d.id.contains("failure-mode.other")),
            "单次模式不命中"
        );
    }

    #[test]
    fn t3_low_reliability_rule_and_thresholds() {
        let config = DiagnosisConfig {
            min_runs: 2,
            reliability_threshold: 0.8,
        };
        // 只有 1 个 run：任何模式都不命中（hits≥2）
        let single = vec![result("r1", Some(0.8), Some(0.5), &["boom"])];
        assert!(
            Diagnoser::new(config.clone())
                .diagnose(&single, "s")
                .is_empty()
        );

        let results = vec![
            result("r1", Some(0.8), Some(0.5), &[]),
            result("r2", Some(0.8), Some(0.6), &[]),
        ];
        let diags = Diagnoser::new(config).diagnose(&results, "s");
        assert!(
            diags.iter().any(|d| d.id.contains("low-reliability")),
            "T3 命中"
        );
    }

    #[test]
    fn diagnosis_to_proposal_and_human_review() {
        let config = DiagnosisConfig::default();
        let results = vec![
            result("r1", None, Some(1.0), &[]),
            result("r2", None, Some(1.0), &[]),
        ];
        let diags = Diagnoser::new(config).diagnose(&results, "fix-java-bug");
        let d = diags
            .iter()
            .find(|d| d.id.contains("missing-test-evidence"))
            .unwrap();
        let mut p = propose_from_diagnosis(d, "fix-java-bug", "v1").unwrap();
        assert_eq!(p.status, MutationStatus::Proposed, "提案等待人工评审");
        assert_eq!(p.evidence, vec!["r1", "r2"]);
        // Human Review：通过 → VALIDATED；评审记录可审计
        p.review_approve("developer-1").unwrap();
        assert_eq!(p.status, MutationStatus::Validated);
        assert!(
            p.promotion_reason
                .as_deref()
                .unwrap()
                .contains("developer-1")
        );
        // 拒绝路径
        let mut p2 = propose_from_diagnosis(d, "fix-java-bug", "v1").unwrap();
        p2.review_reject("developer-1", "draft 步骤名需要细化")
            .unwrap();
        assert_eq!(p2.status, MutationStatus::Rejected);
        assert!(p2.promotion_reason.as_deref().unwrap().contains("细化"));
    }

    #[tokio::test]
    async fn run_diagnosis_persists_idempotently() {
        let storage = crate::storage::LibSQLStorage::open(":memory:")
            .await
            .unwrap();
        let results = vec![
            result("r1", None, Some(1.0), &[]),
            result("r2", None, Some(1.0), &[]),
        ];
        for r in &results {
            storage.save_evaluation(r).await.unwrap();
        }
        let config = DiagnosisConfig::default();
        let first = run_diagnosis(&storage, "fix-java-bug", "v1", None, &config)
            .await
            .unwrap();
        assert!(!first.is_empty(), "诊断应产出提案");
        assert_eq!(
            storage.list_mutations(None).await.unwrap().len(),
            first.len()
        );
        // 新提案 = PROPOSED（等待人工评审，Proposal ≠ Mutation）
        let all: Vec<MutationProposal> = storage.list_mutations(None).await.unwrap();
        assert!(all.iter().all(|p| p.status == MutationStatus::Proposed));

        // 幂等：再次运行不重复提案（仍 PROPOSED → upsert 刷新）
        let second = run_diagnosis(&storage, "fix-java-bug", "v1", None, &config)
            .await
            .unwrap();
        assert_eq!(second.len(), first.len());
        assert_eq!(
            storage.list_mutations(None).await.unwrap().len(),
            first.len()
        );

        // 评审拒绝后再次诊断 → 跳过（REJECTED 决定持久，不被覆盖）
        let mut p = first.into_iter().next().unwrap();
        p.review_reject("developer-1", "不需要").unwrap();
        storage.save_mutation(&p).await.unwrap();
        let third = run_diagnosis(&storage, "fix-java-bug", "v1", None, &config)
            .await
            .unwrap();
        assert!(third.is_empty(), "已评审提案不重新生成");
        let after: Vec<MutationProposal> = storage.list_mutations(None).await.unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].status, MutationStatus::Rejected, "否决决定持久");
    }
}
