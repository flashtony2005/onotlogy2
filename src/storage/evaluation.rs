//! Phase 11 存储：wb_evaluation（派生结果表）+ wb_mutation（提案 + Benchmark）。
//!
//! 派生语义：wb_evaluation 可删表经 Evaluation Reducer 重建——Event Log 永远是 Source of Truth。
//! 11A 证据系统：evidence 列存每维度 `{ evidence: [EventRef], confidence }`，Unknown ≠ Success。

use std::collections::BTreeMap;

use libsql::params;

use super::*;
use crate::evaluation::{EvaluationResult, Score};
use crate::mutation::{MutationProposal, MutationStatus};
use serde_json::{Value, json};

impl LibSQLStorage {
    /// 保存评测结果（幂等 upsert：`eval.<run_id>`；同 run 重算覆盖）。
    pub async fn save_evaluation(&self, r: &EvaluationResult) -> Result<(), StorageError> {
        let id = format!("eval.{}", r.run_id);
        let modes = serde_json::to_string(&r.failure_modes)?;
        let evidence = serde_json::to_string(&evidence_map(r))?;
        self.conn
            .execute(
                "INSERT OR REPLACE INTO wb_evaluation \
                 (id, run_id, task_id, skill_version, correctness, completeness, efficiency, \
                  reliability, policy_compliance, output_quality, aggregate_score, failure_modes, evidence, evaluator_version) \
                 VALUES (?1,?2,?3,NULL,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
                params![
                    id,
                    r.run_id.clone(),
                    r.task_id.clone(),
                    r.correctness.value,
                    r.completeness.value,
                    r.efficiency.value,
                    r.reliability.value,
                    r.policy_compliance.value,
                    r.output_quality.value,
                    r.aggregate,
                    modes,
                    evidence,
                    r.evaluator_version.clone()
                ],
            )
            .await?;
        Ok(())
    }

    /// 查询评测结果（run 过滤可选；升序；派生表——永远可经 Reducer 重建）。
    pub async fn list_evaluations(
        &self,
        run_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<EvaluationResult>, StorageError> {
        let mut sql = String::from(
            "SELECT run_id, task_id, skill_version, correctness, completeness, efficiency, \
             reliability, policy_compliance, output_quality, aggregate_score, failure_modes, evidence, evaluator_version \
             FROM wb_evaluation WHERE 1=1",
        );
        let mut params: Vec<libsql::Value> = Vec::new();
        if let Some(r) = run_id {
            sql.push_str(" AND run_id = ?");
            params.push(libsql::Value::Text(r.to_string()));
        }
        sql.push_str(&format!(
            " ORDER BY created_at ASC LIMIT {}",
            limit.clamp(1, 1000)
        ));

        let mut rows = self
            .conn
            .query(&sql, libsql::params_from_iter(params))
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_evaluation(&row)?);
        }
        Ok(out)
    }

    /// 保存 Mutation 提案（幂等 upsert：`mutation.<target>.<candidate_version>`）。
    pub async fn save_mutation(&self, p: &MutationProposal) -> Result<(), StorageError> {
        let evidence = serde_json::to_string(&p.evidence)?;
        let delta = serde_json::to_string(&p.delta)?;
        // 提案通过结构校验后记 validation_result；Benchmark 判定后记 benchmark_result（判定理由）。
        let validation_result = match p.status {
            MutationStatus::Proposed => None,
            _ => Some("ok".to_string()),
        };
        let benchmark_result = match p.status {
            MutationStatus::Experimental | MutationStatus::Rejected => p.promotion_reason.clone(),
            _ => None,
        };
        self.conn
            .execute(
                "INSERT OR REPLACE INTO wb_mutation \
                 (id, target_asset, source_version, candidate_version, root_cause, evidence, delta, \
                  validation_result, benchmark_result, status, promotion_reason) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                params![
                    p.id.clone(),
                    p.target_asset.clone(),
                    p.source_version.clone(),
                    p.candidate_version.clone(),
                    p.root_cause.clone(),
                    evidence,
                    delta,
                    validation_result,
                    benchmark_result,
                    p.status.as_str(),
                    p.promotion_reason.clone()
                ],
            )
            .await?;
        Ok(())
    }

    /// 查询 Mutation 提案（按状态过滤可选）。
    pub async fn list_mutations(
        &self,
        status: Option<MutationStatus>,
    ) -> Result<Vec<MutationProposal>, StorageError> {
        let mut sql = String::from(
            "SELECT id, target_asset, source_version, candidate_version, root_cause, evidence, delta, status, promotion_reason \
             FROM wb_mutation WHERE 1=1",
        );
        let mut params: Vec<libsql::Value> = Vec::new();
        if let Some(s) = status {
            sql.push_str(" AND status = ?");
            params.push(libsql::Value::Text(s.as_str().to_string()));
        }
        sql.push_str(" ORDER BY created_at ASC");

        let mut rows = self
            .conn
            .query(&sql, libsql::params_from_iter(params))
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_mutation(&row)?);
        }
        Ok(out)
    }
}

/// 每维度 `{ evidence: [EventRef], confidence }`（BTreeMap 保证键序确定性）。
fn evidence_map(r: &EvaluationResult) -> BTreeMap<&'static str, Value> {
    let mut m = BTreeMap::new();
    for (name, score) in [
        ("correctness", &r.correctness),
        ("completeness", &r.completeness),
        ("efficiency", &r.efficiency),
        ("reliability", &r.reliability),
        ("policy_compliance", &r.policy_compliance),
        ("output_quality", &r.output_quality),
    ] {
        m.insert(
            name,
            json!({
                "evidence": score.evidence,
                "confidence": score.confidence,
            }),
        );
    }
    m
}

/// 从 evidence JSON 还原 Score（value 来自列；缺失条目 → unknown）。
fn score_from(value: Option<f64>, entry: Option<&Value>) -> Score {
    match entry {
        Some(e) => Score {
            value,
            evidence: e
                .get("evidence")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default(),
            confidence: e.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.0),
        },
        None => Score::unknown(),
    }
}

fn row_to_evaluation(row: &libsql::Row) -> Result<EvaluationResult, StorageError> {
    let modes: String = row.get(10)?;
    let evidence: String = row.get(11)?;
    let map: BTreeMap<String, Value> = serde_json::from_str(&evidence)?;
    Ok(EvaluationResult {
        run_id: row.get(0)?,
        task_id: row.get(1)?,
        correctness: score_from(row.get(3)?, map.get("correctness")),
        completeness: score_from(row.get(4)?, map.get("completeness")),
        efficiency: score_from(row.get(5)?, map.get("efficiency")),
        reliability: score_from(row.get(6)?, map.get("reliability")),
        policy_compliance: score_from(row.get(7)?, map.get("policy_compliance")),
        output_quality: score_from(row.get(8)?, map.get("output_quality")),
        aggregate: row.get(9)?,
        failure_modes: serde_json::from_str(&modes)?,
        evaluator_version: row.get(12)?,
    })
}

fn row_to_mutation(row: &libsql::Row) -> Result<MutationProposal, StorageError> {
    let evidence: String = row.get(5)?;
    let delta: String = row.get(6)?;
    let status: String = row.get(7)?;
    Ok(MutationProposal {
        id: row.get(0)?,
        target_asset: row.get(1)?,
        source_version: row.get(2)?,
        candidate_version: row.get(3)?,
        root_cause: row.get(4)?,
        evidence: serde_json::from_str(&evidence)?,
        delta: serde_json::from_str(&delta)?,
        status: parse_status(&status),
        promotion_reason: row.get(8)?,
    })
}

fn parse_status(s: &str) -> MutationStatus {
    match s {
        "PROPOSED" => MutationStatus::Proposed,
        "VALIDATED" => MutationStatus::Validated,
        "BENCHMARKING" => MutationStatus::Benchmarking,
        "EXPERIMENTAL" => MutationStatus::Experimental,
        "ACTIVE" => MutationStatus::Active,
        "REJECTED" => MutationStatus::Rejected,
        "ARCHIVED" => MutationStatus::Archived,
        _ => MutationStatus::Proposed,
    }
}
