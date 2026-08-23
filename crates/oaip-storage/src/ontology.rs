use crate::events::{append_trace_tx, is_audit_chain_error};
use crate::{LibSQLStorage, StorageError};
use libsql::params;
use oaip_abi::{OntologyIR, validate_object_data};
use oaip_knowledge::{
    Ontology, OntologyChange, OntologyProposal, OntologyReview, OntologyReviewDecision,
    RuntimeObjectFact, RuntimeRelationFact, canonical_runtime_ontology, relation_endpoints,
    validate_canonical_ontology_with_world, validate_runtime_ir,
};
use oaip_trace::{TraceEvent, TraceKind};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Immutable knowledge-ontology version persisted alongside its optional executable runtime projection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OntologyVersionRecord {
    pub revision: i64,
    pub id: String,
    pub parent_id: Option<String>,
    pub ontology: Ontology,
    pub runtime_ir: Option<OntologyIR>,
    pub created_at: String,
}

/// Lifecycle derived from the durable review and application columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OntologyEvolutionStatus {
    Pending,
    Approved,
    Rejected,
    Applied,
}

impl OntologyEvolutionStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Applied => "applied",
        }
    }

    fn from_str(value: &str) -> Result<Self, StorageError> {
        match value {
            "pending" => Ok(Self::Pending),
            "approved" => Ok(Self::Approved),
            "rejected" => Ok(Self::Rejected),
            "applied" => Ok(Self::Applied),
            _ => Err(governance(format!("未知本体提案状态 {value}"))),
        }
    }
}

/// Durable proposal lineage, including its original semantic impact and named review.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OntologyEvolutionRecord {
    pub id: String,
    pub source_version_id: String,
    pub proposal: OntologyProposal,
    pub review: OntologyReview,
    pub candidate_runtime_ir: Option<OntologyIR>,
    pub applied_version_id: Option<String>,
    pub created_at: String,
    pub reviewed_at: Option<String>,
}

fn governance(message: impl Into<String>) -> StorageError {
    StorageError::Constraint {
        code: "WB2200",
        message: message.into(),
    }
}

fn knowledge(error: oaip_knowledge::KnowledgeError) -> StorageError {
    governance(error.to_string())
}

fn now() -> String {
    oaip_trace::now_ts()
}

fn evolution_event(
    event_type: &'static str,
    proposal_id: &str,
    input: serde_json::Value,
    output: Option<serde_json::Value>,
) -> TraceEvent {
    TraceEvent {
        id: format!("onto_{}", oaip_trace::new_trace_id()),
        kind: TraceKind::Cognitive,
        event_type: event_type.to_string(),
        object_refs: vec![proposal_id.to_owned()],
        project_id: None,
        run_id: None,
        agent_id: None,
        reasoning: Some("ontology evolution governance".to_owned()),
        input: Some(input),
        output,
        confidence: None,
        timestamp: now(),
    }
}

async fn append_evolution_event_tx(
    storage: &LibSQLStorage,
    tx: &libsql::Transaction,
    event_type: &'static str,
    proposal_id: &str,
    input: serde_json::Value,
    output: Option<serde_json::Value>,
) -> Result<(), StorageError> {
    if let Err(error) =
        append_trace_tx(tx, &evolution_event(event_type, proposal_id, input, output)).await
    {
        if is_audit_chain_error(&error) {
            storage.mark_audit_chain_invalid();
        }
        return Err(error);
    }
    Ok(())
}

fn row_to_version(row: &libsql::Row) -> Result<OntologyVersionRecord, StorageError> {
    let ontology: String = row.get(3)?;
    let runtime_ir: Option<String> = row.get(4)?;
    Ok(OntologyVersionRecord {
        revision: row.get(0)?,
        id: row.get(1)?,
        parent_id: row.get(2)?,
        ontology: serde_json::from_str(&ontology)?,
        runtime_ir: runtime_ir.map(|ir| serde_json::from_str(&ir)).transpose()?,
        created_at: row.get(5)?,
    })
}

fn row_to_proposal(row: &libsql::Row) -> Result<OntologyEvolutionRecord, StorageError> {
    let proposal_json: String = row.get(2)?;
    let review_json: String = row.get(3)?;
    let status = OntologyEvolutionStatus::from_str(&row.get::<String>(4)?)?;
    let applied_version_id: Option<String> = row.get(5)?;
    let candidate_runtime_ir: Option<String> = row.get(6)?;
    let review: OntologyReview = serde_json::from_str(&review_json)?;
    let expected = match (review.decision(), applied_version_id.is_some()) {
        (OntologyReviewDecision::Pending, false) => OntologyEvolutionStatus::Pending,
        (OntologyReviewDecision::Approved, false) => OntologyEvolutionStatus::Approved,
        (OntologyReviewDecision::Rejected, false) => OntologyEvolutionStatus::Rejected,
        (OntologyReviewDecision::Approved, true) => OntologyEvolutionStatus::Applied,
        _ => {
            return Err(governance(format!(
                "本体提案 {} 的评审状态与应用状态不一致",
                row.get::<String>(0)?
            )));
        }
    };
    if status != expected {
        return Err(governance(format!(
            "本体提案 {} 状态列损坏",
            row.get::<String>(0)?
        )));
    }
    let proposal: OntologyProposal = serde_json::from_str(&proposal_json)?;
    if proposal.id() != row.get::<String>(0)? {
        return Err(governance("本体提案 ID 与载荷不一致"));
    }
    if review.proposal_id() != proposal.id() {
        return Err(governance("本体评审 ID 与提案不一致"));
    }
    Ok(OntologyEvolutionRecord {
        id: row.get(0)?,
        source_version_id: row.get(1)?,
        proposal,
        review,
        candidate_runtime_ir: candidate_runtime_ir
            .map(|ir| serde_json::from_str(&ir))
            .transpose()?,
        applied_version_id,
        created_at: row.get(7)?,
        reviewed_at: row.get(8)?,
    })
}

async fn version_tx(
    tx: &libsql::Transaction,
    id: &str,
) -> Result<Option<OntologyVersionRecord>, StorageError> {
    let mut rows = tx .query( "SELECT revision,id,parent_id,ontology,runtime_ir,created_at  FROM wb_ontology_version WHERE id=?1", params![id], ) .await?;
    rows.next()
        .await?
        .map(|row| row_to_version(&row))
        .transpose()
}

async fn proposal_tx(
    tx: &libsql::Transaction,
    id: &str,
) -> Result<Option<OntologyEvolutionRecord>, StorageError> {
    let mut rows = tx .query( "SELECT id,source_version_id,proposal_json,review_json,status,applied_version_id, candidate_runtime_ir,created_at,reviewed_at  FROM wb_ontology_proposal WHERE id=?1", params![id], ) .await?;
    rows.next()
        .await?
        .map(|row| row_to_proposal(&row))
        .transpose()
}

async fn insert_version_tx(
    tx: &libsql::Transaction,
    id: &str,
    parent_id: Option<&str>,
    ontology: &Ontology,
    runtime_ir: Option<&OntologyIR>,
    created_at: &str,
) -> Result<OntologyVersionRecord, StorageError> {
    let ontology_json = serde_json::to_string(ontology)?;
    let runtime_ir = runtime_ir.map(serde_json::to_string).transpose()?;
    tx.execute( "INSERT INTO wb_ontology_version (id,parent_id,ontology,runtime_ir,created_at) VALUES (?1,?2,?3,?4,?5)", params![id, parent_id, ontology_json, runtime_ir, created_at], ) .await?;
    version_tx(tx, id)
        .await?
        .ok_or_else(|| governance("本体版本插入后不可读取"))
}

async fn insert_proposal_tx(
    tx: &libsql::Transaction,
    record: &OntologyEvolutionRecord,
) -> Result<(), StorageError> {
    tx.execute( "INSERT INTO wb_ontology_proposal  (id,source_version_id,proposal_json,review_json,candidate_runtime_ir,status,applied_version_id,created_at,reviewed_at)  VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![ record.id.clone(), record.source_version_id.clone(), serde_json::to_string(&record.proposal)?, serde_json::to_string(&record.review)?, record .candidate_runtime_ir .as_ref() .map(serde_json::to_string) .transpose()?, status_for_record(record).as_str(), record.applied_version_id.clone(), record.created_at.clone(), record.reviewed_at.clone(), ], ) .await?;
    Ok(())
}

fn status_for_record(record: &OntologyEvolutionRecord) -> OntologyEvolutionStatus {
    if record.applied_version_id.is_some() {
        OntologyEvolutionStatus::Applied
    } else {
        match record.review.decision() {
            OntologyReviewDecision::Pending => OntologyEvolutionStatus::Pending,
            OntologyReviewDecision::Approved => OntologyEvolutionStatus::Approved,
            OntologyReviewDecision::Rejected => OntologyEvolutionStatus::Rejected,
        }
    }
}

async fn update_review_tx(
    tx: &libsql::Transaction,
    record: &OntologyEvolutionRecord,
    reviewed_at: &str,
) -> Result<(), StorageError> {
    let updated = tx .execute( "UPDATE wb_ontology_proposal SET review_json=?1,status=?2,reviewed_at=?3  WHERE id=?4 AND status='pending'", params![ serde_json::to_string(&record.review)?, status_for_record(record).as_str(), reviewed_at, record.id.clone(), ], ) .await?;
    if updated != 1 {
        return Err(governance(format!("本体提案 {} 不再是 pending", record.id)));
    }
    Ok(())
}

async fn objects_tx(tx: &libsql::Transaction) -> Result<Vec<super::ObjectRecord>, StorageError> {
    let mut rows = tx
        .query(
            "SELECT id, type, data, version FROM wb_ontology_object WHERE is_delete=0",
            params![],
        )
        .await?;
    let mut objects = Vec::new();
    while let Some(row) = rows.next().await? {
        objects.push(super::row_to_object(&row)?);
    }
    Ok(objects)
}

async fn validate_runtime_ir_against_world(
    tx: &libsql::Transaction,
    runtime_ir: &OntologyIR,
) -> Result<(), StorageError> {
    validate_runtime_ir(runtime_ir).map_err(knowledge)?;
    for object in objects_tx(tx).await? {
        let Some(definition) = runtime_ir.find(&object.r#type) else {
            continue;
        };
        if let Err(error) = validate_object_data(definition, &object.data) {
            return Err(governance(format!(
                "对象 {} 与候选运行时本体不兼容: {error}",
                object.id
            )));
        }
    }
    let endpoints = relation_endpoints(runtime_ir);
    let object_types = objects_tx(tx)
        .await?
        .into_iter()
        .map(|object| (object.id, object.r#type))
        .collect::<std::collections::HashMap<_, _>>();
    for edge in relations_tx(tx).await? {
        let Some((source_type, target_type)) = endpoints.get(&edge.relation) else {
            return Err(governance(format!(
                "关系 {} 未在候选运行时本体中声明",
                edge.relation
            )));
        };
        for (role, actual_id, actual_type, expected_type) in [
            (
                "source",
                &edge.source,
                object_types.get(&edge.source),
                source_type,
            ),
            (
                "target",
                &edge.target,
                object_types.get(&edge.target),
                target_type,
            ),
        ] {
            let Some(actual_type) = actual_type else {
                return Err(governance(format!(
                    "关系 {} 的 {role} {} 不存在",
                    edge.relation, actual_id
                )));
            };
            if actual_type != expected_type {
                return Err(governance(format!(
                    "关系 {} 的 {role} {} 类型是 {actual_type}，期望 {expected_type}",
                    edge.relation, actual_id
                )));
            }
        }
    }
    Ok(())
}

async fn relations_tx(
    tx: &libsql::Transaction,
) -> Result<Vec<super::RelationRecord>, StorageError> {
    let mut rows = tx
        .query(
            "SELECT id, relation, source, target, data FROM wb_relation_edge WHERE is_delete=0",
            params![],
        )
        .await?;
    let mut relations = Vec::new();
    while let Some(row) = rows.next().await? {
        relations.push(super::row_to_relation(&row)?);
    }
    Ok(relations)
}

async fn validate_canonical_in_transaction(
    tx: &libsql::Transaction,
    ontology: &Ontology,
) -> Result<(), StorageError> {
    let objects = objects_tx(tx)
        .await?
        .into_iter()
        .map(|object| RuntimeObjectFact {
            id: object.id,
            object_type: object.r#type,
            data: object.data,
        })
        .collect::<Vec<_>>();
    let relations = relations_tx(tx)
        .await?
        .into_iter()
        .map(|relation| RuntimeRelationFact {
            source: relation.source,
            relation: relation.relation,
            target: relation.target,
        })
        .collect::<Vec<_>>();
    validate_canonical_ontology_with_world(ontology, &objects, &relations).map_err(knowledge)
}

impl LibSQLStorage {
    pub async fn initialize_knowledge_version(
        &self,
        id: impl Into<String>,
        ontology: &Ontology,
        runtime_ir: Option<&OntologyIR>,
    ) -> Result<OntologyVersionRecord, StorageError> {
        let id = id.into();
        if id.trim().is_empty() {
            return Err(governance("本体版本 ID 不能为空"));
        }
        let _write_txn = self.write_txn_lock.lock().await;
        let tx = self.conn.transaction().await?;
        if version_tx(&tx, &id).await?.is_some()
            || self.current_knowledge_version().await?.is_some()
        {
            tx.rollback().await?;
            return Err(governance("知识本体已经初始化"));
        }
        let mut persisted_ontology = ontology.clone();
        if let Some(runtime_ir) = runtime_ir {
            validate_runtime_ir_against_world(&tx, runtime_ir).await?;
            persisted_ontology = canonical_runtime_ontology(ontology, None, Some(runtime_ir));
            oaip_knowledge::validate_canonical_ontology(&persisted_ontology).map_err(knowledge)?;
        }
        validate_canonical_in_transaction(&tx, &persisted_ontology).await?;
        let created_at = now();
        let version =
            insert_version_tx(&tx, &id, None, &persisted_ontology, runtime_ir, &created_at).await?;
        append_evolution_event_tx(
            self,
            &tx,
            "OntologyInitialized",
            &version.id,
            json!({ "versionId": version.id }),
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(version)
    }

    pub async fn propose_knowledge_change(
        &self,
        source_version_id: &str,
        proposal: &OntologyProposal,
        candidate_runtime_ir: Option<&OntologyIR>,
    ) -> Result<OntologyEvolutionRecord, StorageError> {
        let _write_txn = self.write_txn_lock.lock().await;
        let tx = self.conn.transaction().await?;
        if proposal.id().trim().is_empty() {
            return Err(governance("本体提案 ID 不能为空"));
        }
        if proposal_tx(&tx, proposal.id()).await?.is_some() {
            return Err(governance(format!("本体提案 {} 已存在", proposal.id())));
        }
        let Some(source) = version_tx(&tx, source_version_id).await? else {
            return Err(StorageError::NotFound(source_version_id.to_owned()));
        };
        let proposed_target = proposal.target(&source.ontology).map_err(knowledge)?;
        if let Some(runtime_ir) = candidate_runtime_ir {
            validate_runtime_ir_against_world(&tx, runtime_ir).await?;
            let preview = canonical_runtime_ontology(
                &proposed_target,
                source.runtime_ir.as_ref(),
                Some(runtime_ir),
            );
            validate_canonical_in_transaction(&tx, &preview).await?;
        } else {
            if let Some(runtime_ir) = source.runtime_ir.as_ref() {
                validate_runtime_ir_against_world(&tx, runtime_ir).await?;
            }
            validate_canonical_in_transaction(&tx, &proposed_target).await?;
        }
        let review = oaip_knowledge::validate_ontology_proposal(proposal, &source.ontology)
            .map_err(knowledge)?;
        let record = OntologyEvolutionRecord {
            id: proposal.id().to_owned(),
            source_version_id: source.id.clone(),
            proposal: proposal.clone(),
            review,
            candidate_runtime_ir: candidate_runtime_ir.cloned(),
            applied_version_id: None,
            created_at: now(),
            reviewed_at: None,
        };
        insert_proposal_tx(&tx, &record).await?;
        append_evolution_event_tx( self, &tx, "OntologyProposalCreated", &record.id.clone(), json!({ "sourceVersionId": source.id, "admissible": record.review.is_admissible(), "lossy": record.review.is_lossy(), }), None, ) .await?;
        tx.commit().await?;
        Ok(record)
    }

    pub async fn decide_knowledge_proposal(
        &self,
        id: &str,
        approve: bool,
        reviewer: &str,
        reason: Option<&str>,
    ) -> Result<OntologyEvolutionRecord, StorageError> {
        if reviewer.trim().is_empty() {
            return Err(governance("评审人不能为空"));
        }
        let _write_txn = self.write_txn_lock.lock().await;
        let tx = self.conn.transaction().await?;
        let mut record = proposal_tx(&tx, id)
            .await?
            .ok_or_else(|| StorageError::NotFound(id.to_owned()))?;
        if record.review.decision() != OntologyReviewDecision::Pending {
            return Err(governance(format!("本体提案 {} 已完成评审", id)));
        }
        let source = version_tx(&tx, &record.source_version_id)
            .await?
            .ok_or_else(|| governance(format!("本体提案 {} 的源版本缺失", id)))?;
        record.review =
            oaip_knowledge::validate_ontology_proposal(&record.proposal, &source.ontology)
                .map_err(knowledge)?;
        if approve {
            record.review.approve(reviewer).map_err(knowledge)?;
        } else {
            record
                .review
                .reject(reviewer, reason.unwrap_or("unspecified"))
                .map_err(knowledge)?;
        }
        let reviewed_at = now();
        update_review_tx(&tx, &record, &reviewed_at).await?;
        record.reviewed_at = Some(reviewed_at);
        append_evolution_event_tx(
            self,
            &tx,
            if approve {
                "OntologyProposalApproved"
            } else {
                "OntologyProposalRejected"
            },
            &record.id.clone(),
            json!({ "reviewer": reviewer, "reason": record.review.reason() }),
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(record)
    }

    pub async fn apply_knowledge_proposal(
        &self,
        id: &str,
        target_version_id: impl Into<String>,
    ) -> Result<(OntologyEvolutionRecord, OntologyVersionRecord), StorageError> {
        let target_id = target_version_id.into();
        if target_id.trim().is_empty() {
            return Err(governance("目标本体版本 ID 不能为空"));
        }
        let _write_txn = self.write_txn_lock.lock().await;
        let tx = self.conn.transaction().await?;
        let mut record = proposal_tx(&tx, id)
            .await?
            .ok_or_else(|| StorageError::NotFound(id.to_owned()))?;
        if record.review.decision() != OntologyReviewDecision::Approved
            || !record.review.is_approved()
            || record.review.reviewer().is_none()
        {
            return Err(governance(format!("本体提案 {} 尚未获得具名批准", id)));
        }
        if version_tx(&tx, &target_id).await?.is_some() {
            return Err(governance(format!("目标本体版本 {target_id} 已存在")));
        }
        let source = version_tx(&tx, &record.source_version_id)
            .await?
            .ok_or_else(|| governance(format!("本体提案 {} 的源版本缺失", id)))?;
        let fresh_review =
            oaip_knowledge::validate_ontology_proposal(&record.proposal, &source.ontology)
                .map_err(knowledge)?;
        if fresh_review.impact() != record.review.impact() {
            return Err(governance(format!("本体提案 {} 的影响面已变化", id)));
        }
        let applied_ontology = oaip_knowledge::apply_ontology_proposal(
            &record.proposal,
            &record.review,
            &source.ontology,
        )
        .map_err(knowledge)?;
        let active_runtime_ir = record
            .candidate_runtime_ir
            .clone()
            .or_else(|| source.runtime_ir.clone());
        if let Some(runtime_ir) = active_runtime_ir.as_ref() {
            validate_runtime_ir_against_world(&tx, runtime_ir).await?;
        }
        let target_ontology = canonical_runtime_ontology(
            &applied_ontology,
            source.runtime_ir.as_ref(),
            active_runtime_ir.as_ref(),
        );
        validate_canonical_in_transaction(&tx, &target_ontology).await?;
        let version = insert_version_tx(
            &tx,
            &target_id,
            Some(&source.id),
            &target_ontology,
            active_runtime_ir.as_ref(),
            &now(),
        )
        .await?;
        let updated = tx .execute( "UPDATE wb_ontology_proposal SET status='applied',applied_version_id=?1  WHERE id=?2 AND status='approved'", params![version.id.clone(), record.id.clone()], ) .await?;
        if updated != 1 {
            return Err(governance(format!("本体提案 {} 不能重复应用", id)));
        }
        record.applied_version_id = Some(version.id.clone());
        append_evolution_event_tx(
            self,
            &tx,
            "OntologyProposalApplied",
            &record.id.clone(),
            json!({ "reviewer": record.review.reviewer(), "sourceVersionId": source.id, }),
            Some(json!({ "targetVersionId": version.id })),
        )
        .await?;
        tx.commit().await?;
        Ok((record, version))
    }

    pub async fn propose_knowledge_rollback(
        &self,
        proposal_id: impl Into<String>,
        target_version_id: &str,
    ) -> Result<OntologyEvolutionRecord, StorageError> {
        let proposal_id = proposal_id.into();
        let proposal = OntologyProposal::new(proposal_id);
        let current = self
            .current_knowledge_version()
            .await?
            .ok_or_else(|| governance("尚无可回滚的知识本体"))?;
        let target = self
            .knowledge_version(target_version_id)
            .await?
            .ok_or_else(|| StorageError::NotFound(target_version_id.to_owned()))?;
        if current.id == target.id {
            return Err(governance("当前版本已是回滚目标"));
        }
        let proposal = proposal.with_change(OntologyChange::Replace(target.ontology.clone()));
        self.propose_knowledge_change(&current.id, &proposal, target.runtime_ir.as_ref())
            .await
    }

    pub async fn knowledge_version(
        &self,
        id: &str,
    ) -> Result<Option<OntologyVersionRecord>, StorageError> {
        let mut rows = self .conn .query( "SELECT revision,id,parent_id,ontology,runtime_ir,created_at  FROM wb_ontology_version WHERE id=?1", params![id], ) .await?;
        rows.next()
            .await?
            .map(|row| row_to_version(&row))
            .transpose()
    }

    pub async fn current_knowledge_version(
        &self,
    ) -> Result<Option<OntologyVersionRecord>, StorageError> {
        let mut rows = self .conn .query( "SELECT revision,id,parent_id,ontology,runtime_ir,created_at  FROM wb_ontology_version ORDER BY revision DESC LIMIT 1", params![], ) .await?;
        rows.next()
            .await?
            .map(|row| row_to_version(&row))
            .transpose()
    }

    pub async fn list_knowledge_versions(
        &self,
    ) -> Result<Vec<OntologyVersionRecord>, StorageError> {
        let mut rows = self .conn .query( "SELECT revision,id,parent_id,ontology,runtime_ir,created_at  FROM wb_ontology_version ORDER BY revision ASC", params![], ) .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_version(&row)?);
        }
        Ok(out)
    }

    pub async fn knowledge_proposal(
        &self,
        id: &str,
    ) -> Result<Option<OntologyEvolutionRecord>, StorageError> {
        let mut rows = self .conn .query( "SELECT id,source_version_id,proposal_json,review_json,status,applied_version_id, candidate_runtime_ir,created_at,reviewed_at  FROM wb_ontology_proposal WHERE id=?1", params![id], ) .await?;
        rows.next()
            .await?
            .map(|row| row_to_proposal(&row))
            .transpose()
    }

    pub async fn list_knowledge_proposals(
        &self,
    ) -> Result<Vec<OntologyEvolutionRecord>, StorageError> {
        let mut rows = self .conn .query( "SELECT id,source_version_id,proposal_json,review_json,status,applied_version_id, candidate_runtime_ir,created_at,reviewed_at  FROM wb_ontology_proposal ORDER BY created_at ASC,id ASC", params![], ) .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_proposal(&row)?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LibSQLStorage, ObjectEngine, WorldCommand};
    use oaip_abi::ir::{ObjectType, PropertyDef, RelationDef, ScalarType};
    use oaip_knowledge::{
        Axiom, ClassExpression, DataFacet, DataRange, Datatype, FacetValue, Literal, Term,
    };

    fn class(name: &str) -> ClassExpression {
        ClassExpression::class(format!("http://example.com/owl#{name}"))
    }

    fn root() -> Ontology {
        Ontology::new().with_axiom(Axiom::subclass(class("Dog"), class("Animal")))
    }

    #[tokio::test]
    async fn evolution_versions_decisions_and_rollback_are_persisted() {
        let path = std::env::temp_dir().join(format!(
            "oaip-ontology-evolution-{}-{}.db",
            std::process::id(),
            oaip_trace::now_ts()
        ));
        let _ = std::fs::remove_file(&path);
        let storage = LibSQLStorage::open(path.to_str().unwrap()).await.unwrap();
        let first = storage
            .initialize_knowledge_version("root", &root(), None)
            .await
            .unwrap();
        assert_eq!((1, "root".to_owned()), (first.revision, first.id));
        let proposal = OntologyProposal::new("add-puppy").with_change(OntologyChange::Add(
            Axiom::subclass(class("Puppy"), class("Dog")),
        ));
        let pending = storage
            .propose_knowledge_change("root", &proposal, None)
            .await
            .unwrap();
        assert_eq!(
            OntologyEvolutionStatus::Pending,
            status_for_record(&pending)
        );
        assert!(
            storage
                .apply_knowledge_proposal("add-puppy", "v2")
                .await
                .is_err()
        );
        let approved = storage
            .decide_knowledge_proposal("add-puppy", true, "alice", None)
            .await
            .unwrap();
        assert_eq!("alice", approved.review.reviewer().unwrap());
        let (_, second) = storage
            .apply_knowledge_proposal("add-puppy", "v2")
            .await
            .unwrap();
        assert_eq!(Some("root".to_owned()), second.parent_id);
        let rollback = storage
            .propose_knowledge_rollback("rollback", "root")
            .await
            .unwrap();
        assert_eq!("v2", rollback.source_version_id);
        storage
            .decide_knowledge_proposal("rollback", true, "bob", Some("restore baseline"))
            .await
            .unwrap();
        let (_, third) = storage
            .apply_knowledge_proposal("rollback", "v3")
            .await
            .unwrap();
        assert_eq!(root(), third.ontology);
        let rejection = OntologyProposal::new("remove-dog-animal").with_change(
            OntologyChange::Remove(Axiom::subclass(class("Dog"), class("Animal"))),
        );
        storage
            .propose_knowledge_change("v3", &rejection, None)
            .await
            .unwrap();
        let rejected = storage
            .decide_knowledge_proposal(
                "remove-dog-animal",
                false,
                "carol",
                Some("baseline must keep dogs as animals"),
            )
            .await
            .unwrap();
        assert_eq!(OntologyReviewDecision::Rejected, rejected.review.decision());
        assert_eq!(
            Some("baseline must keep dogs as animals"),
            rejected.review.reason()
        );
        drop(storage);
        let reopened = LibSQLStorage::open(path.to_str().unwrap()).await.unwrap();
        let versions = reopened.list_knowledge_versions().await.unwrap();
        assert_eq!(
            vec!["root", "v2", "v3"],
            versions.iter().map(|v| v.id.as_str()).collect::<Vec<_>>()
        );
        assert_eq!(
            "v3",
            reopened
                .current_knowledge_version()
                .await
                .unwrap()
                .unwrap()
                .id
        );
        assert_eq!(
            1,
            reopened
                .current_knowledge_version()
                .await
                .unwrap()
                .unwrap()
                .ontology
                .len()
        );
        let applied = reopened
            .knowledge_proposal("add-puppy")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(Some("v2"), applied.applied_version_id.as_deref());
        assert!(reopened.validate_audit_chain().await.is_ok());
        let events = reopened.list_all_events().await.unwrap();
        for expected in [
            "OntologyInitialized",
            "OntologyProposalCreated",
            "OntologyProposalApproved",
            "OntologyProposalApplied",
        ] {
            assert!(events.iter().any(|event| event.event_type == expected));
        }
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn runtime_ir_evolution_validates_world_and_persists_lineage() {
        fn customer_ir(properties: Vec<PropertyDef>) -> OntologyIR {
            OntologyIR {
                relations: Vec::new(),
                disjoint_groups: Vec::new(),
                equivalent_groups: Vec::new(),
                object_types: vec![ObjectType {
                    name: "Customer".to_owned(),
                    extends: Vec::new(),
                    properties,
                    actions: Vec::new(),
                }],
            }
        }
        fn name_property(ty: ScalarType) -> PropertyDef {
            PropertyDef {
                name: "name".to_owned(),
                ty,
                constraints: Vec::new(),
            }
        }
        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        storage
            .create(
                "Customer",
                "customer-1",
                serde_json::json!({ "name": "Ada" }),
            )
            .await
            .unwrap();
        let root_ir = customer_ir(vec![name_property(ScalarType::String)]);
        storage
            .initialize_knowledge_version("root", &root(), Some(&root_ir))
            .await
            .unwrap();
        let incompatible = customer_ir(vec![name_property(ScalarType::Int)]);
        let bad = OntologyProposal::new("bad-runtime");
        assert!(
            storage
                .propose_knowledge_change("root", &bad, Some(&incompatible))
                .await
                .is_err()
        );
        let compatible = customer_ir(vec![
            name_property(ScalarType::String),
            PropertyDef {
                name: "score".to_owned(),
                ty: ScalarType::Int,
                constraints: Vec::new(),
            },
        ]);
        let proposal = OntologyProposal::new("add-score").with_change(OntologyChange::Add(
            Axiom::subclass(class("Puppy"), class("Dog")),
        ));
        storage
            .propose_knowledge_change("root", &proposal, Some(&compatible))
            .await
            .unwrap();
        storage
            .decide_knowledge_proposal("add-score", true, "alice", None)
            .await
            .unwrap();
        let (_, v2) = storage
            .apply_knowledge_proposal("add-score", "v2")
            .await
            .unwrap();
        assert_eq!(Some(compatible.clone()), v2.runtime_ir);
        let pure_dl = OntologyProposal::new("pure-dl").with_change(OntologyChange::Add(
            Axiom::subclass(class("Puppy"), class("Dog")),
        ));
        storage
            .propose_knowledge_change("v2", &pure_dl, None)
            .await
            .unwrap();
        storage
            .decide_knowledge_proposal("pure-dl", true, "bob", None)
            .await
            .unwrap();
        let (_, v3) = storage
            .apply_knowledge_proposal("pure-dl", "v3")
            .await
            .unwrap();
        assert_eq!(Some(compatible), v3.runtime_ir);
        storage
            .propose_knowledge_rollback("rollback-root", "root")
            .await
            .unwrap();
        storage
            .decide_knowledge_proposal("rollback-root", true, "carol", None)
            .await
            .unwrap();
        let (_, restored) = storage
            .apply_knowledge_proposal("rollback-root", "v4")
            .await
            .unwrap();
        assert_eq!(Some(root_ir), restored.runtime_ir);
    }

    #[tokio::test]
    async fn string_length_restriction_gates_live_world_evolution() {
        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        let root =
            Ontology::new().with_axiom(Axiom::subclass(class("Customer"), ClassExpression::Thing));
        storage
            .initialize_knowledge_version("root", &root, None)
            .await
            .unwrap();
        storage
            .create(
                "Customer",
                "customer-1",
                serde_json::json!({ "name": "Ada" }),
            )
            .await
            .unwrap();

        let admissible = Axiom::DataPropertyRangeRestriction {
            property: Term::new("name"),
            datatype: Datatype::String,
            facets: vec![
                FacetValue {
                    facet: DataFacet::MinLength,
                    value: Literal::Integer(2),
                },
                FacetValue {
                    facet: DataFacet::MaxLength,
                    value: Literal::Integer(5),
                },
            ],
        };
        let proposal = OntologyProposal::new("add-name-length")
            .with_change(OntologyChange::Add(admissible.clone()));
        storage
            .propose_knowledge_change("root", &proposal, None)
            .await
            .unwrap();
        storage
            .decide_knowledge_proposal("add-name-length", true, "alice", None)
            .await
            .unwrap();
        storage
            .apply_knowledge_proposal("add-name-length", "v2")
            .await
            .unwrap();

        // Intersecting the applied range down to [2,2] excludes the live fact.
        let incompatible = Axiom::DataPropertyRangeRestriction {
            property: Term::new("name"),
            datatype: Datatype::String,
            facets: vec![FacetValue {
                facet: DataFacet::MaxLength,
                value: Literal::Integer(2),
            }],
        };
        let bad = OntologyProposal::new("exclude-live-name")
            .with_change(OntologyChange::Add(incompatible));
        assert!(
            storage
                .propose_knowledge_change("v2", &bad, None)
                .await
                .is_err()
        );
        let version = storage
            .knowledge_version("v2")
            .await
            .unwrap()
            .expect("applied version exists");
        assert!(version.ontology.axioms().contains(&admissible));
    }

    #[tokio::test]
    async fn string_pattern_gates_live_world_evolution() {
        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        let root =
            Ontology::new().with_axiom(Axiom::subclass(class("Customer"), ClassExpression::Thing));
        storage
            .initialize_knowledge_version("root", &root, None)
            .await
            .unwrap();
        storage
            .create("Customer", "customer-1", json!({ "name": "Ada" }))
            .await
            .unwrap();

        fn name_pattern(pattern: &str) -> Axiom {
            Axiom::DataPropertyRangeRestriction {
                property: Term::new("name"),
                datatype: Datatype::String,
                facets: vec![FacetValue {
                    facet: DataFacet::Pattern,
                    value: Literal::String(pattern.to_owned()),
                }],
            }
        }

        let supported = OntologyProposal::new("reject-inline-flags")
            .with_change(OntologyChange::Add(name_pattern("(?i)ada")));
        assert!(
            storage
                .propose_knowledge_change("root", &supported, None)
                .await
                .is_err()
        );

        let admissible = name_pattern("[A-Z][a-z]+");
        let proposal = OntologyProposal::new("add-name-pattern")
            .with_change(OntologyChange::Add(admissible.clone()));
        storage
            .propose_knowledge_change("root", &proposal, None)
            .await
            .unwrap();
        storage
            .decide_knowledge_proposal("add-name-pattern", true, "alice", None)
            .await
            .unwrap();
        storage
            .apply_knowledge_proposal("add-name-pattern", "v2")
            .await
            .unwrap();

        // This syntactically valid narrowing excludes the persisted live fact,
        // so governance must reject it before creating a new version.
        let incompatible = OntologyProposal::new("exclude-live-name")
            .with_change(OntologyChange::Add(name_pattern("Bob.*")));
        assert!(
            storage
                .propose_knowledge_change("v2", &incompatible, None)
                .await
                .is_err()
        );
        let version = storage
            .knowledge_version("v2")
            .await
            .unwrap()
            .expect("applied version exists");
        assert!(version.ontology.axioms().contains(&admissible));
    }

    #[tokio::test]
    async fn datatype_restriction_gates_live_world_evolution() {
        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        let root =
            Ontology::new().with_axiom(Axiom::subclass(class("Customer"), ClassExpression::Thing));
        storage
            .initialize_knowledge_version("root", &root, None)
            .await
            .unwrap();
        storage
            .create("Customer", "customer-1", serde_json::json!({ "age": 30 }))
            .await
            .unwrap();

        let admissible = Axiom::DataPropertyRangeRestriction {
            property: Term::new("age"),
            datatype: Datatype::Integer,
            facets: vec![
                FacetValue {
                    facet: DataFacet::MinInclusive,
                    value: Literal::Integer(18),
                },
                FacetValue {
                    facet: DataFacet::MaxInclusive,
                    value: Literal::Integer(65),
                },
            ],
        };
        let proposal = OntologyProposal::new("add-age-range")
            .with_change(OntologyChange::Add(admissible.clone()));
        storage
            .propose_knowledge_change("root", &proposal, None)
            .await
            .unwrap();
        storage
            .decide_knowledge_proposal("add-age-range", true, "alice", None)
            .await
            .unwrap();
        storage
            .apply_knowledge_proposal("add-age-range", "v2")
            .await
            .unwrap();

        // The new range intersects the applied one and excludes the live fact.
        let incompatible = Axiom::DataPropertyRangeRestriction {
            property: Term::new("age"),
            datatype: Datatype::Integer,
            facets: vec![FacetValue {
                facet: DataFacet::MinInclusive,
                value: Literal::Integer(31),
            }],
        };
        let bad = OntologyProposal::new("exclude-live-age")
            .with_change(OntologyChange::Add(incompatible));
        assert!(
            storage
                .propose_knowledge_change("v2", &bad, None)
                .await
                .is_err()
        );
        let version = storage
            .knowledge_version("v2")
            .await
            .unwrap()
            .expect("applied version exists");
        assert!(version.ontology.axioms().contains(&admissible));
    }

    #[tokio::test]
    async fn composable_data_range_gates_live_world_evolution() {
        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        let root =
            Ontology::new().with_axiom(Axiom::subclass(class("Customer"), ClassExpression::Thing));
        storage
            .initialize_knowledge_version("root", &root, None)
            .await
            .unwrap();
        storage
            .create(
                "Customer",
                "customer-1",
                serde_json::json!({ "contact": "support@example.com" }),
            )
            .await
            .unwrap();

        let admissible = Axiom::DataPropertyDataRange {
            property: Term::new("contact"),
            range: DataRange::union([
                DataRange::restriction(
                    Datatype::String,
                    vec![FacetValue {
                        facet: DataFacet::Pattern,
                        value: Literal::String("[^@]+@[^@]+".into()),
                    }],
                ),
                DataRange::Datatype(Datatype::Integer),
            ]),
        };
        let proposal = OntologyProposal::new("add-contact-union")
            .with_change(OntologyChange::Add(admissible.clone()));
        storage
            .propose_knowledge_change("root", &proposal, None)
            .await
            .unwrap();
        storage
            .decide_knowledge_proposal("add-contact-union", true, "alice", None)
            .await
            .unwrap();
        storage
            .apply_knowledge_proposal("add-contact-union", "v2")
            .await
            .unwrap();

        // Intersecting the union branches excludes the persisted string fact.
        let incompatible = OntologyProposal::new("exclude-live-contact").with_change(
            OntologyChange::Add(Axiom::DataPropertyDataRange {
                property: Term::new("contact"),
                range: DataRange::intersection([
                    DataRange::Datatype(Datatype::Integer),
                    DataRange::restriction(
                        Datatype::Integer,
                        vec![FacetValue {
                            facet: DataFacet::MinInclusive,
                            value: Literal::Integer(1),
                        }],
                    ),
                ]),
            }),
        );
        assert!(
            storage
                .propose_knowledge_change("v2", &incompatible, None)
                .await
                .is_err()
        );
        let version = storage
            .knowledge_version("v2")
            .await
            .unwrap()
            .expect("applied version exists");
        assert!(version.ontology.axioms().contains(&admissible));
    }

    #[tokio::test]
    async fn data_property_enumeration_gates_live_world_evolution() {
        fn one_of(values: &[&str]) -> Axiom {
            Axiom::DataPropertyOneOf {
                property: Term::new("status"),
                values: values
                    .iter()
                    .map(|value| Literal::String((*value).into()))
                    .collect(),
            }
        }

        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        let root =
            Ontology::new().with_axiom(Axiom::subclass(class("Customer"), ClassExpression::Thing));
        storage
            .initialize_knowledge_version("root", &root, None)
            .await
            .unwrap();
        storage
            .create(
                "Customer",
                "customer-1",
                serde_json::json!({ "status": "Active" }),
            )
            .await
            .unwrap();

        let broad = one_of(&["Active", "Pending"]);
        let proposal =
            OntologyProposal::new("add-status").with_change(OntologyChange::Add(broad.clone()));
        storage
            .propose_knowledge_change("root", &proposal, None)
            .await
            .unwrap();
        storage
            .decide_knowledge_proposal("add-status", true, "alice", None)
            .await
            .unwrap();
        storage
            .apply_knowledge_proposal("add-status", "v2")
            .await
            .unwrap();

        // Enumerations are ordinary governed axioms and can be narrowed.
        let narrow = OntologyProposal::new("narrow-status")
            .with_change(OntologyChange::Remove(broad))
            .with_change(OntologyChange::Add(one_of(&["Active"])));
        storage
            .propose_knowledge_change("v2", &narrow, None)
            .await
            .unwrap();
        storage
            .decide_knowledge_proposal("narrow-status", true, "bob", None)
            .await
            .unwrap();
        storage
            .apply_knowledge_proposal("narrow-status", "v3")
            .await
            .unwrap();

        // The live ABox participates immediately: narrowing past the existing
        // fact is rejected before a proposal can be persisted.
        let incompatible = OntologyProposal::new("exclude-active")
            .with_change(OntologyChange::Add(one_of(&["Pending"])));
        assert!(
            storage
                .propose_knowledge_change("v3", &incompatible, None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn date_time_runtime_properties_validate_live_world_and_evolution() {
        let ir = OntologyIR {
            relations: Vec::new(),
            disjoint_groups: Vec::new(),
            equivalent_groups: Vec::new(),
            object_types: vec![ObjectType {
                name: "Event".to_owned(),
                extends: Vec::new(),
                properties: vec![PropertyDef {
                    name: "occurred_at".to_owned(),
                    ty: ScalarType::DateTime,
                    constraints: Vec::new(),
                }],
                actions: Vec::new(),
            }],
        };
        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        storage
            .create(
                "Event",
                "event-1",
                serde_json::json!({ "occurred_at": "2026-08-23T08:00:00+08:00" }),
            )
            .await
            .unwrap();
        let proposal = OntologyProposal::new("add-event").with_change(OntologyChange::Add(
            Axiom::subclass(class("Scheduled"), class("Event")),
        ));
        assert!(
            storage
                .propose_knowledge_change("missing", &proposal, Some(&ir))
                .await
                .is_err()
        );
        storage
            .initialize_knowledge_version("root", &Ontology::new(), Some(&ir))
            .await
            .unwrap();
        storage
            .propose_knowledge_change("root", &proposal, None)
            .await
            .unwrap();
        storage
            .decide_knowledge_proposal("add-event", true, "alice", None)
            .await
            .unwrap();
        let (_, version) = storage
            .apply_knowledge_proposal("add-event", "v2")
            .await
            .unwrap();
        assert_eq!(Some(ir), version.runtime_ir);
    }

    #[tokio::test]
    async fn date_runtime_properties_validate_live_world_and_evolution() {
        let ir = OntologyIR {
            relations: Vec::new(),
            disjoint_groups: Vec::new(),
            equivalent_groups: Vec::new(),
            object_types: vec![ObjectType {
                name: "Person".to_owned(),
                extends: Vec::new(),
                properties: vec![PropertyDef {
                    name: "birthday".to_owned(),
                    ty: ScalarType::Date,
                    constraints: Vec::new(),
                }],
                actions: Vec::new(),
            }],
        };
        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        storage
            .create("Person", "person-1", json!({ "birthday": "2024-02-29" }))
            .await
            .unwrap();
        storage
            .initialize_knowledge_version("root", &Ontology::new(), Some(&ir))
            .await
            .unwrap();

        // The live fact excludes a proposal that would narrow the value space.
        let incompatible = OntologyProposal::new("before-leap-year").with_change(
            OntologyChange::Add(Axiom::DataPropertyRangeRestriction {
                property: Term::new("birthday"),
                datatype: Datatype::Date,
                facets: vec![FacetValue {
                    facet: DataFacet::MaxExclusive,
                    value: Literal::Date("2024-01-01".into()),
                }],
            }),
        );
        assert!(
            storage
                .propose_knowledge_change("root", &incompatible, None)
                .await
                .is_err()
        );

        let compatible = OntologyProposal::new("add-person").with_change(OntologyChange::Add(
            Axiom::subclass(class("Minor"), class("Person")),
        ));
        storage
            .propose_knowledge_change("root", &compatible, None)
            .await
            .unwrap();
        storage
            .decide_knowledge_proposal("add-person", true, "alice", None)
            .await
            .unwrap();
        let (_, version) = storage
            .apply_knowledge_proposal("add-person", "v2")
            .await
            .unwrap();
        assert_eq!(Some(ir), version.runtime_ir);
    }

    #[tokio::test]
    async fn decimal_runtime_properties_validate_live_world_and_evolution() {
        let ir = OntologyIR {
            relations: Vec::new(),
            disjoint_groups: Vec::new(),
            equivalent_groups: Vec::new(),
            object_types: vec![ObjectType {
                name: "Account".to_owned(),
                extends: Vec::new(),
                properties: vec![PropertyDef {
                    name: "balance".to_owned(),
                    ty: ScalarType::Decimal,
                    constraints: Vec::new(),
                }],
                actions: Vec::new(),
            }],
        };
        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        storage
            .create(
                "Account",
                "account-1",
                json!({ "balance": "999999999999999999999999.5000" }),
            )
            .await
            .unwrap();
        storage
            .initialize_knowledge_version("root", &Ontology::new(), Some(&ir))
            .await
            .unwrap();

        // A bound below the live balance fails closed before proposal persistence.
        let incompatible = OntologyProposal::new("below-live-balance").with_change(
            OntologyChange::Add(Axiom::DataPropertyRangeRestriction {
                property: Term::new("balance"),
                datatype: Datatype::Decimal,
                facets: vec![FacetValue {
                    facet: DataFacet::MaxExclusive,
                    value: Literal::Decimal("999999999999999999999999.49999".into()),
                }],
            }),
        );
        assert!(
            storage
                .propose_knowledge_change("root", &incompatible, None)
                .await
                .is_err()
        );

        let compatible = OntologyProposal::new("add-account").with_change(OntologyChange::Add(
            Axiom::subclass(class("Frozen"), class("Account")),
        ));
        storage
            .propose_knowledge_change("root", &compatible, None)
            .await
            .unwrap();
        storage
            .decide_knowledge_proposal("add-account", true, "alice", None)
            .await
            .unwrap();
        let (_, version) = storage
            .apply_knowledge_proposal("add-account", "v2")
            .await
            .unwrap();
        assert_eq!(Some(ir), version.runtime_ir);
    }

    #[tokio::test]
    async fn odl_property_constraints_gate_initialization_and_evolution() {
        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        let root = Ontology::new().with_axiom(Axiom::subclass(class("Dog"), class("Animal")));
        let customer = |constraints: &str| {
            oaip_abi::parse_odl(&format!(
                r#"object Customer:
  properties:
    name: string {constraints}
"#
            ))
        };
        let valid_ir = customer("min_length 2 pattern \"[A-Z][a-z]+\"").unwrap();
        storage
            .create(
                "Customer",
                "customer-1",
                serde_json::json!({ "name": "Ada" }),
            )
            .await
            .unwrap();
        storage
            .initialize_knowledge_version("root", &root, Some(&valid_ir))
            .await
            .unwrap();

        // The executable schema is materialized into the persisted DL version,
        // not merely stored beside it as an untyped annotation.
        let initialized = storage.knowledge_version("root").await.unwrap().unwrap();
        assert!(initialized.ontology.axioms().iter().any(|axiom| matches!(
            axiom,
            Axiom::DataPropertyRangeRestriction { property, datatype: Datatype::String, facets }
                if property.as_str() == "name"
                    && facets.iter().any(|facet| facet.facet == DataFacet::Pattern)
        )));

        // A narrowing that excludes the already-persisted live fact is rejected
        // before a proposal/version can be created.
        let incompatible_ir = customer("min_length 5").unwrap();
        let bad = OntologyProposal::new("exclude-live-name");
        assert!(
            storage
                .propose_knowledge_change("root", &bad, Some(&incompatible_ir))
                .await
                .is_err()
        );

        // A compatible reviewed replacement persists both lineage and runtime IR.
        let compatible_ir = customer("min_length 2 pattern \"[A-Z][a-z]+\"").unwrap();
        let proposal = OntologyProposal::new("constrain-name").with_change(OntologyChange::Add(
            Axiom::subclass(class("Puppy"), class("Dog")),
        ));
        storage
            .propose_knowledge_change("root", &proposal, Some(&compatible_ir.clone()))
            .await
            .unwrap();
        storage
            .decide_knowledge_proposal("constrain-name", true, "alice", None)
            .await
            .unwrap();
        let (_, applied) = storage
            .apply_knowledge_proposal("constrain-name", "v2")
            .await
            .unwrap();
        assert_eq!(Some(compatible_ir), applied.runtime_ir);
    }

    #[tokio::test]
    async fn odl_scalar_cardinality_gates_initialization_and_evolution() {
        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        let root = Ontology::new().with_axiom(Axiom::subclass(class("Dog"), class("Animal")));
        let customer = |constraints: &str| {
            oaip_abi::parse_odl(&format!(
                r#"object Customer:
  properties:
    name: string {constraints}
"#
            ))
            .unwrap()
        };
        storage
            .create("Customer", "customer-1", serde_json::json!({}))
            .await
            .unwrap();

        // Required fields are executable narrowing decisions, so initialization
        // cannot bless a schema that already excludes a live row.
        let required = customer("min_cardinality 1");
        assert!(
            storage
                .initialize_knowledge_version("root", &root, Some(&required))
                .await
                .is_err()
        );

        // An optional schema initializes, and a later required-field narrowing
        // is rejected by the same live-world governance gate.
        storage
            .initialize_knowledge_version("root", &root, Some(&customer("")))
            .await
            .unwrap();
        let proposal = OntologyProposal::new("require-name");
        assert!(
            storage
                .propose_knowledge_change("root", &proposal, Some(&required))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn odl_extends_gates_and_replaces_taxonomy_projection() {
        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        let source = r#"
object Animal:
  properties:
    name: string
object Pet extends Animal:
  properties:
    note: string
object Dog extends Animal:
  properties:
    breed: string
"#;
        let initial_ir = oaip_abi::parse_odl(source).unwrap();
        storage
            .create(
                "Dog",
                "dog-1",
                serde_json::json!({ "name": "Ada", "note": "friendly", "breed": "lab" }),
            )
            .await
            .unwrap();
        storage
            .initialize_knowledge_version("root", &Ontology::new(), Some(&initial_ir))
            .await
            .unwrap();
        let initialized = storage.knowledge_version("root").await.unwrap().unwrap();
        assert!(initialized.ontology.axioms().contains(&Axiom::subclass(
            ClassExpression::class(Term::new("Dog")),
            ClassExpression::class(Term::new("Animal"))
        )));

        let mut replacement_ir = oaip_abi::parse_odl(source).unwrap();
        replacement_ir.object_types[2].extends = vec!["Pet".to_owned()];
        let proposal = OntologyProposal::new("move-dog-under-pet").with_change(
            OntologyChange::Add(Axiom::subclass(
                ClassExpression::class(Term::new("Puppy")),
                ClassExpression::class(Term::new("Dog")),
            )),
        );
        storage
            .propose_knowledge_change("root", &proposal, Some(&replacement_ir))
            .await
            .unwrap();
        storage
            .decide_knowledge_proposal("move-dog-under-pet", true, "alice", None)
            .await
            .unwrap();
        let (_, applied) = storage
            .apply_knowledge_proposal("move-dog-under-pet", "v2")
            .await
            .unwrap();

        assert!(applied.ontology.axioms().contains(&Axiom::subclass(
            ClassExpression::class(Term::new("Dog")),
            ClassExpression::class(Term::new("Pet"))
        )));
        assert!(!applied.ontology.axioms().contains(&Axiom::subclass(
            ClassExpression::class(Term::new("Dog")),
            ClassExpression::class(Term::new("Animal"))
        )));
        assert_eq!(Some(replacement_ir), applied.runtime_ir);
    }

    #[tokio::test]
    async fn odl_class_groups_gate_live_world_and_replace_exactly() {
        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        let base = r#"
object Animal:
  properties:
    name: string
object Dog extends Animal:
  properties:
    breed: string
object Cat extends Animal:
  properties:
    indoor: bool
"#;
        let initial_ir = oaip_abi::parse_odl(base).unwrap();
        storage
            .create(
                "Dog",
                "dog-1",
                serde_json::json!({ "name": "Ada", "breed": "lab" }),
            )
            .await
            .unwrap();
        storage
            .initialize_knowledge_version("root", &Ontology::new(), Some(&initial_ir))
            .await
            .unwrap();

        // Narrowing the live Dog into a disjoint hierarchy is rejected before a
        // pending proposal can exist.
        let mut incompatible_ir = oaip_abi::parse_odl(base).unwrap();
        incompatible_ir.disjoint_groups = vec![vec!["Dog".to_owned(), "Animal".to_owned()]];
        let incompatible = OntologyProposal::new("make-live-dog-unsatisfiable").with_change(
            OntologyChange::Add(Axiom::subclass(
                ClassExpression::class(Term::new("Puppy")),
                ClassExpression::class(Term::new("Dog")),
            )),
        );
        assert!(
            storage
                .propose_knowledge_change("root", &incompatible, Some(&incompatible_ir))
                .await
                .is_err()
        );
        assert!(storage.list_knowledge_proposals().await.unwrap().is_empty());

        // A disjoint boundary over distinct concrete types persists as DL and
        // then removes exactly when its executable declaration disappears.
        let mut disjoint_ir = oaip_abi::parse_odl(base).unwrap();
        disjoint_ir.disjoint_groups = vec![vec!["Dog".to_owned(), "Cat".to_owned()]];
        let add_disjoint = OntologyProposal::new("separate-dog-from-cat").with_change(
            OntologyChange::Add(Axiom::subclass(
                ClassExpression::class(Term::new("Puppy")),
                ClassExpression::class(Term::new("Dog")),
            )),
        );
        storage
            .propose_knowledge_change("root", &add_disjoint, Some(&disjoint_ir))
            .await
            .unwrap();
        storage
            .decide_knowledge_proposal("separate-dog-from-cat", true, "alice", None)
            .await
            .unwrap();
        let (_, applied) = storage
            .apply_knowledge_proposal("separate-dog-from-cat", "v2")
            .await
            .unwrap();
        assert!(
            applied
                .ontology
                .axioms()
                .contains(&Axiom::DisjointClasses(vec![
                    ClassExpression::class(Term::new("Dog")),
                    ClassExpression::class(Term::new("Cat"))
                ]))
        );

        let removed_ir = oaip_abi::parse_odl(base).unwrap();
        let remove_disjoint = OntologyProposal::new("remove-disjoint-boundary").with_change(
            OntologyChange::Add(Axiom::subclass(
                ClassExpression::class(Term::new("Friendly")),
                ClassExpression::class(Term::new("Dog")),
            )),
        );
        storage
            .propose_knowledge_change("v2", &remove_disjoint, Some(&removed_ir))
            .await
            .unwrap();
        storage
            .decide_knowledge_proposal("remove-disjoint-boundary", true, "bob", None)
            .await
            .unwrap();
        let (_, removed) = storage
            .apply_knowledge_proposal("remove-disjoint-boundary", "v3")
            .await
            .unwrap();
        assert!(
            !removed
                .ontology
                .axioms()
                .contains(&Axiom::DisjointClasses(vec![
                    ClassExpression::class(Term::new("Dog")),
                    ClassExpression::class(Term::new("Cat"))
                ]))
        );
        assert_eq!(Some(removed_ir), removed.runtime_ir);
    }

    #[tokio::test]
    async fn pure_dl_evolution_validates_the_current_live_world() {
        fn named(name: &str) -> ClassExpression {
            ClassExpression::class(Term::new(name))
        }
        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        let root = Ontology::new()
            .with_axiom(Axiom::subclass(named("Customer"), ClassExpression::Thing))
            .with_axiom(Axiom::subclass(named("Order"), ClassExpression::Thing));
        storage
            .initialize_knowledge_version("root", &root, None)
            .await
            .unwrap();
        storage
            .apply_world(WorldCommand::CreateObject {
                object_type: "Customer".to_owned(),
                id: "c1".to_owned(),
                data: serde_json::json!({}),
                project_id: None,
            })
            .await
            .unwrap();
        let proposal = OntologyProposal::new("bad-unsatisfiable-world").with_change(
            OntologyChange::Add(Axiom::subclass(named("Customer"), ClassExpression::Nothing)),
        );
        assert!(
            storage
                .propose_knowledge_change("root", &proposal, None)
                .await
                .is_err()
        );
        assert!(storage.list_knowledge_proposals().await.unwrap().is_empty());
        assert_eq!(1, storage.list_knowledge_versions().await.unwrap().len());
    }

    #[tokio::test]
    async fn runtime_relations_validate_live_source_and_target_types() {
        fn relation_ir(target: &str) -> OntologyIR {
            let object = |name: &str| ObjectType {
                name: name.to_owned(),
                extends: Vec::new(),
                properties: vec![PropertyDef {
                    name: "name".to_owned(),
                    ty: ScalarType::String,
                    constraints: Vec::new(),
                }],
                actions: Vec::new(),
            };
            OntologyIR {
                object_types: vec![object("Customer"), object("Order")],
                relations: vec![RelationDef {
                    name: "placed".to_owned(),
                    source: "Customer".to_owned(),
                    target: target.to_owned(),
                    ..RelationDef::default()
                }],
                disjoint_groups: Vec::new(),
                equivalent_groups: Vec::new(),
            }
        }
        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        storage
            .initialize_knowledge_version("root", &root(), None)
            .await
            .unwrap();
        storage
            .apply_world(WorldCommand::CreateObject {
                object_type: "Customer".to_owned(),
                id: "c1".to_owned(),
                data: serde_json::json!({}),
                project_id: None,
            })
            .await
            .unwrap();
        storage
            .apply_world(WorldCommand::CreateObject {
                object_type: "Customer".to_owned(),
                id: "o1".to_owned(),
                data: serde_json::json!({}),
                project_id: None,
            })
            .await
            .unwrap();
        storage
            .apply_world(WorldCommand::LinkRelation {
                source: "c1".to_owned(),
                edge: "placed".to_owned(),
                target: "o1".to_owned(),
                project_id: None,
            })
            .await
            .unwrap();
        let proposal = OntologyProposal::new("bad-relation-world");
        assert!(
            storage
                .propose_knowledge_change("root", &proposal, Some(&relation_ir("Order")))
                .await
                .is_err()
        );
        assert!(storage.list_knowledge_proposals().await.unwrap().is_empty());
        storage
            .apply_world(WorldCommand::UnlinkRelation {
                source: "c1".to_owned(),
                edge: "placed".to_owned(),
                target: "o1".to_owned(),
                project_id: None,
            })
            .await
            .unwrap();
        storage.delete("o1").await.unwrap();
        storage
            .create("Order", "o2", serde_json::json!({}))
            .await
            .unwrap();
        storage
            .apply_world(WorldCommand::LinkRelation {
                source: "c1".to_owned(),
                edge: "placed".to_owned(),
                target: "o2".to_owned(),
                project_id: None,
            })
            .await
            .unwrap();
        let proposal = OntologyProposal::new("good-relation-world");
        storage
            .propose_knowledge_change("root", &proposal, Some(&relation_ir("Order")))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn pure_dl_proposals_validate_the_inherited_runtime_relations() {
        fn object(name: &str) -> ObjectType {
            ObjectType {
                name: name.to_owned(),
                extends: Vec::new(),
                properties: vec![PropertyDef {
                    name: "name".to_owned(),
                    ty: ScalarType::String,
                    constraints: Vec::new(),
                }],
                actions: Vec::new(),
            }
        }
        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        let runtime_ir = OntologyIR {
            object_types: vec![object("Customer"), object("Order")],
            relations: vec![RelationDef {
                name: "placed".to_owned(),
                source: "Customer".to_owned(),
                target: "Order".to_owned(),
                ..RelationDef::default()
            }],
            disjoint_groups: Vec::new(),
            equivalent_groups: Vec::new(),
        };
        storage
            .initialize_knowledge_version("root", &root(), Some(&runtime_ir))
            .await
            .unwrap();
        storage
            .apply_world(WorldCommand::CreateObject {
                object_type: "Customer".to_owned(),
                id: "c1".to_owned(),
                data: serde_json::json!({}),
                project_id: None,
            })
            .await
            .unwrap();
        storage
            .apply_world(WorldCommand::CreateObject {
                object_type: "Customer".to_owned(),
                id: "bad-target".to_owned(),
                data: serde_json::json!({}),
                project_id: None,
            })
            .await
            .unwrap();
        storage
            .apply_world(WorldCommand::LinkRelation {
                source: "c1".to_owned(),
                edge: "placed".to_owned(),
                target: "bad-target".to_owned(),
                project_id: None,
            })
            .await
            .unwrap();
        let proposal = OntologyProposal::new("pure-dl-preview").with_change(OntologyChange::Add(
            Axiom::subclass(class("Puppy"), class("Dog")),
        ));
        assert!(
            storage
                .propose_knowledge_change("root", &proposal, None)
                .await
                .is_err()
        );
        assert!(storage.list_knowledge_proposals().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn functional_relation_violations_fail_closed_with_live_world() {
        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        let object = |name: &str| ObjectType {
            name: name.to_owned(),
            extends: Vec::new(),
            properties: vec![PropertyDef {
                name: "name".to_owned(),
                ty: ScalarType::String,
                constraints: Vec::new(),
            }],
            actions: Vec::new(),
        };
        let runtime_ir = OntologyIR {
            object_types: vec![object("Customer"), object("Order")],
            relations: vec![RelationDef {
                name: "placed".to_owned(),
                source: "Customer".to_owned(),
                target: "Order".to_owned(),
                functional: true,
                ..RelationDef::default()
            }],
            disjoint_groups: Vec::new(),
            equivalent_groups: Vec::new(),
        };
        storage
            .initialize_knowledge_version("root", &root(), Some(&runtime_ir))
            .await
            .unwrap();
        for (object_type, id) in [("Customer", "c1"), ("Order", "o1"), ("Order", "o2")] {
            storage
                .apply_world(WorldCommand::CreateObject {
                    object_type: object_type.to_owned(),
                    id: id.to_owned(),
                    data: serde_json::json!({}),
                    project_id: None,
                })
                .await
                .unwrap();
        }
        for target in ["o1", "o2"] {
            storage
                .apply_world(WorldCommand::LinkRelation {
                    source: "c1".to_owned(),
                    edge: "placed".to_owned(),
                    target: target.to_owned(),
                    project_id: None,
                })
                .await
                .unwrap();
        }
        let proposal = OntologyProposal::new("functional-world").with_change(OntologyChange::Add(
            Axiom::subclass(class("Puppy"), class("Dog")),
        ));
        assert!(
            storage
                .propose_knowledge_change("root", &proposal, None)
                .await
                .is_err()
        );
        assert!(storage.list_knowledge_proposals().await.unwrap().is_empty());
        assert_eq!(1, storage.list_knowledge_versions().await.unwrap().len());
    }

    #[tokio::test]
    async fn relation_cardinality_violations_fail_closed_with_live_world() {
        let source = r#"
object Customer:
  properties:
    name: string
object Order:
  properties:
    total: int
relation placed:
  source: Customer
  target: Order
"#;
        fn runtime_ir(min: u32, max: u32) -> OntologyIR {
            let mut ir = oaip_abi::parse_odl(
                r#"
object Customer:
  properties:
    name: string
object Order:
  properties:
    total: int
relation placed:
  source: Customer
  target: Order
"#,
            )
            .unwrap();
            ir.relations[0].min_cardinality = Some(min);
            ir.relations[0].max_cardinality = Some(max);
            ir
        }
        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        storage
            .initialize_knowledge_version(
                "root",
                &root(),
                Some(&oaip_abi::parse_odl(source).unwrap()),
            )
            .await
            .unwrap();
        for (object_type, id) in [("Customer", "c1"), ("Order", "o1"), ("Order", "o2")] {
            storage
                .apply_world(WorldCommand::CreateObject {
                    object_type: object_type.to_owned(),
                    id: id.to_owned(),
                    data: serde_json::json!({}),
                    project_id: None,
                })
                .await
                .unwrap();
        }
        for target in ["o1", "o2"] {
            storage
                .apply_world(WorldCommand::LinkRelation {
                    source: "c1".to_owned(),
                    edge: "placed".to_owned(),
                    target: target.to_owned(),
                    project_id: None,
                })
                .await
                .unwrap();
        }

        let too_many = runtime_ir(1, 1);
        let proposal = OntologyProposal::new("too-many-orders").with_change(OntologyChange::Add(
            Axiom::subclass(class("Puppy"), class("Dog")),
        ));
        assert!(
            storage
                .propose_knowledge_change("root", &proposal, Some(&too_many))
                .await
                .is_err()
        );

        storage
            .apply_world(WorldCommand::UnlinkRelation {
                source: "c1".to_owned(),
                edge: "placed".to_owned(),
                target: "o2".to_owned(),
                project_id: None,
            })
            .await
            .unwrap();
        let too_few = runtime_ir(2, 2);
        let proposal = OntologyProposal::new("too-few-orders").with_change(OntologyChange::Add(
            Axiom::subclass(class("Puppy"), class("Dog")),
        ));
        assert!(
            storage
                .propose_knowledge_change("root", &proposal, Some(&too_few))
                .await
                .is_err()
        );
        assert!(storage.list_knowledge_proposals().await.unwrap().is_empty());

        let compatible = runtime_ir(1, 2);
        let proposal = OntologyProposal::new("bounded-placed").with_change(OntologyChange::Add(
            Axiom::subclass(class("Puppy"), class("Dog")),
        ));
        storage
            .propose_knowledge_change("root", &proposal, Some(&compatible.clone()))
            .await
            .unwrap();
        storage
            .decide_knowledge_proposal("bounded-placed", true, "alice", None)
            .await
            .unwrap();
        let (_, applied) = storage
            .apply_knowledge_proposal("bounded-placed", "v2")
            .await
            .unwrap();
        assert_eq!(Some(compatible), applied.runtime_ir);
        assert!(applied.ontology.axioms().iter().any(|axiom| matches!(
            axiom,
            Axiom::SubClassOf {
                sup: ClassExpression::ObjectMinCardinality { cardinality: 1, .. },
                ..
            }
        )));
        assert!(applied.ontology.axioms().iter().any(|axiom| matches!(
            axiom,
            Axiom::SubClassOf {
                sup: ClassExpression::ObjectMaxCardinality { cardinality: 2, .. },
                ..
            }
        )));
    }

    #[tokio::test]
    async fn inconsistent_or_unapproved_changes_fail_closed() {
        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        storage
            .initialize_knowledge_version("root", &root(), None)
            .await
            .unwrap();
        let unsat =
            OntologyProposal::new("make-empty").with_change(OntologyChange::Add(Axiom::subclass(
                class("Empty"),
                ClassExpression::intersection([
                    class("Animal"),
                    ClassExpression::complement(class("Animal")),
                ]),
            )));
        storage
            .propose_knowledge_change("root", &unsat, None)
            .await
            .unwrap();
        assert!(
            storage
                .decide_knowledge_proposal("make-empty", true, "alice", None)
                .await
                .is_err()
        );
        let stored = storage
            .knowledge_proposal("make-empty")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(OntologyReviewDecision::Pending, stored.review.decision());
        let normal = OntologyProposal::new("safe-add").with_change(OntologyChange::Add(
            Axiom::subclass(class("Puppy"), class("Dog")),
        ));
        storage
            .propose_knowledge_change("root", &normal, None)
            .await
            .unwrap();
        assert!(
            storage
                .apply_knowledge_proposal("safe-add", "v2")
                .await
                .is_err()
        );
        assert_eq!(1, storage.list_knowledge_versions().await.unwrap().len());
    }

    #[tokio::test]
    async fn integral_float_world_data_allows_governed_evolution() {
        fn named(name: &str) -> ClassExpression {
            ClassExpression::class(Term::new(name))
        }

        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        let root = Ontology::new()
            .with_axiom(Axiom::subclass(named("Customer"), ClassExpression::Thing))
            .with_axiom(Axiom::DataPropertyDomain {
                property: Term::new("score"),
                class: named("Customer"),
            })
            .with_axiom(Axiom::DataPropertyRange {
                property: Term::new("score"),
                datatype: Datatype::Double,
            });
        storage
            .create("Customer", "c1", serde_json::json!({ "score": 30 }))
            .await
            .unwrap();

        // The JSON integer must be projected as the declared canonical double;
        // otherwise initialization correctly fails closed on a range conflict.
        storage
            .initialize_knowledge_version("root", &root, None)
            .await
            .unwrap();

        let proposal = OntologyProposal::new("safe-add").with_change(OntologyChange::Add(
            Axiom::subclass(named("Puppy"), named("Dog")),
        ));
        storage
            .propose_knowledge_change("root", &proposal, None)
            .await
            .unwrap();
        storage
            .decide_knowledge_proposal("safe-add", true, "alice", None)
            .await
            .unwrap();
        let (_, version) = storage
            .apply_knowledge_proposal("safe-add", "v2")
            .await
            .unwrap();
        assert_eq!(Some("root".to_owned()), version.parent_id);
    }

    #[tokio::test]
    async fn datatype_conflicts_fail_closed_with_the_live_world() {
        fn named(name: &str) -> ClassExpression {
            ClassExpression::class(Term::new(name))
        }

        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        let root = Ontology::new()
            .with_axiom(Axiom::subclass(named("Customer"), ClassExpression::Thing))
            .with_axiom(Axiom::SameIndividuals(vec![
                Term::new("c1"),
                Term::new("c2"),
            ]));
        storage
            .initialize_knowledge_version("root", &root, None)
            .await
            .unwrap();

        for (id, age) in [("c1", 30), ("c2", 31)] {
            storage
                .apply_world(WorldCommand::CreateObject {
                    object_type: "Customer".to_owned(),
                    id: id.to_owned(),
                    data: serde_json::json!({ "age": age }),
                    project_id: None,
                })
                .await
                .unwrap();
        }

        let contradictory = OntologyProposal::new("negative-age").with_change(OntologyChange::Add(
            Axiom::NegativeDataPropertyAssertion {
                property: Term::new("age"),
                individual: Term::new("c1"),
                value: Literal::Integer(30),
            },
        ));
        assert!(
            storage
                .propose_knowledge_change("root", &contradictory, None)
                .await
                .is_err()
        );

        let functional = OntologyProposal::new("functional-age").with_change(OntologyChange::Add(
            Axiom::FunctionalDataProperty(Term::new("age")),
        ));
        assert!(
            storage
                .propose_knowledge_change("root", &functional, None)
                .await
                .is_err()
        );

        let range = OntologyProposal::new("integer-age").with_change(OntologyChange::Add(
            Axiom::DataPropertyRange {
                property: Term::new("age"),
                datatype: Datatype::Integer,
            },
        ));
        assert!(
            storage
                .propose_knowledge_change("root", &range, None)
                .await
                .is_err()
        );

        assert!(storage.list_knowledge_proposals().await.unwrap().is_empty());
        assert_eq!(1, storage.list_knowledge_versions().await.unwrap().len());
    }

    #[tokio::test]
    async fn data_domain_entailments_fail_closed_with_disjoint_live_types() {
        fn named(name: &str) -> ClassExpression {
            ClassExpression::class(Term::new(name))
        }

        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        let root = Ontology::new()
            .with_axiom(Axiom::subclass(named("Customer"), ClassExpression::Thing))
            .with_axiom(Axiom::subclass(named("Order"), ClassExpression::Thing))
            .with_axiom(Axiom::DisjointClasses(vec![
                named("Customer"),
                named("Order"),
            ]))
            .with_axiom(Axiom::DataPropertyDomain {
                property: Term::new("age"),
                class: named("Customer"),
            });
        storage
            .initialize_knowledge_version("root", &root, None)
            .await
            .unwrap();

        for (object_type, id) in [("Customer", "c1"), ("Order", "o1")] {
            storage
                .apply_world(WorldCommand::CreateObject {
                    object_type: object_type.to_owned(),
                    id: id.to_owned(),
                    data: serde_json::json!({ "age": 30 }),
                    project_id: None,
                })
                .await
                .unwrap();
        }

        // The Order row's inherited age domain entails Customer, conflicting
        // with the explicit disjoint Customer/Order boundary.
        let safe_elsewhere = OntologyProposal::new("safe-add").with_change(OntologyChange::Add(
            Axiom::subclass(named("Puppy"), named("Dog")),
        ));
        assert!(
            storage
                .propose_knowledge_change("root", &safe_elsewhere, None)
                .await
                .is_err()
        );
        assert!(storage.list_knowledge_proposals().await.unwrap().is_empty());
        assert_eq!(1, storage.list_knowledge_versions().await.unwrap().len());
    }
}
