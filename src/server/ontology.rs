//! 知识本体与受控演化 HTTP Transport。
//!
//! 读操作要求 Operator；初始化、提案、评审、应用和回滚要求 Developer。
//! 评审人始终取认证会话用户，不接受客户端伪造的 reviewer 字段。

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::knowledge::Literal as KnowledgeLiteral;
use crate::knowledge::Term as KnowledgeTerm;

use crate::abi::ir::OntologyIR;
use crate::auth::AuthUser;
use crate::compose::defs::{ir_to_runtime, ir_to_services};
use crate::knowledge::Ontology as KnowledgeOntology;
use crate::knowledge::{
    OntologyProposal, ReasonedJustification, Reasoner, validate_explanation_limit,
};
use crate::knowledge_bridge::{
    storage_world_snapshot, storage_world_snapshot_to_knowledge,
    world_snapshot_cardinality_violations, world_snapshot_data_cardinality_violations,
    world_snapshot_to_knowledge,
};
use crate::storage::StorageError;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde::Deserialize;
use serde_json::json;

use super::{ApiResponse, AppState, forbidden, wb_code};

fn error_response(error: StorageError) -> Response {
    match error {
        StorageError::NotFound(message) => {
            ApiResponse::error(404, format!("不存在：{message}")).into_response()
        }
        StorageError::Constraint { code, message } => {
            // WB codes are stable API failure codes; their HTTP transport status is Bad Request.
            let api_code = code.trim_start_matches("WB").parse().unwrap_or(4000);
            (
                StatusCode::BAD_REQUEST,
                ApiResponse::error(api_code, message),
            )
                .into_response()
        }
        _ => ApiResponse::error(5000, "知识本体存储错误").into_response(),
    }
}

fn write_denied(user: &AuthUser) -> Option<Response> {
    if user.can_write_evolution() {
        None
    } else {
        Some(forbidden("当前角色无权演化知识本体（需 Developer）"))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InitializeKnowledgeBody {
    pub id: String,
    pub ontology: KnowledgeOntology,
    pub runtime_ir: Option<OntologyIR>,
}

pub(crate) async fn initialize_knowledge(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Json(body): Json<InitializeKnowledgeBody>,
) -> Response {
    if let Some(response) = write_denied(&user) {
        return response;
    }
    match st
        .storage
        .initialize_knowledge_version(body.id, &body.ontology, body.runtime_ir.as_ref())
        .await
    {
        Ok(version) => ApiResponse::ok(version).into_response(),
        Err(error) => error_response(error),
    }
}

pub(crate) async fn current_knowledge(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
) -> Response {
    if !user.can_read_evaluation() {
        return forbidden("当前角色无权读取知识本体（需 Operator 及以上）").into_response();
    }
    match st.storage.current_knowledge_version().await {
        Ok(version) => ApiResponse::ok(version).into_response(),
        Err(error) => error_response(error),
    }
}

pub(crate) async fn list_knowledge_versions(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
) -> Response {
    if !user.can_read_evaluation() {
        return forbidden("当前角色无权读取知识本体（需 Operator 及以上）").into_response();
    }
    match st.storage.list_knowledge_versions().await {
        Ok(versions) => {
            ApiResponse::ok(json!({ "total": versions.len(), "list": versions })).into_response()
        }
        Err(error) => error_response(error),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateKnowledgeProposalBody {
    pub source_version_id: String,
    pub proposal: OntologyProposal,
    pub runtime_ir: Option<OntologyIR>,
}

pub(crate) async fn create_knowledge_proposal(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Json(body): Json<CreateKnowledgeProposalBody>,
) -> Response {
    if let Some(response) = write_denied(&user) {
        return response;
    }
    match st
        .storage
        .propose_knowledge_change(
            &body.source_version_id,
            &body.proposal,
            body.runtime_ir.as_ref(),
        )
        .await
    {
        Ok(record) => ApiResponse::ok(record).into_response(),
        Err(error) => error_response(error),
    }
}

pub(crate) async fn list_knowledge_proposals(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
) -> Response {
    if !user.can_read_evaluation() {
        return forbidden("当前角色无权读取知识提案（需 Operator 及以上）").into_response();
    }
    match st.storage.list_knowledge_proposals().await {
        Ok(records) => {
            ApiResponse::ok(json!({ "total": records.len(), "list": records })).into_response()
        }
        Err(error) => error_response(error),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReviewKnowledgeBody {
    pub approve: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

pub(crate) async fn review_knowledge_proposal(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<String>,
    Json(body): Json<ReviewKnowledgeBody>,
) -> Response {
    if let Some(response) = write_denied(&user) {
        return response;
    }
    match st
        .storage
        .decide_knowledge_proposal(&id, body.approve, &user.username, body.reason.as_deref())
        .await
    {
        Ok(record) => ApiResponse::ok(record).into_response(),
        Err(error) => error_response(error),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApplyKnowledgeBody {
    pub target_version_id: String,
}

pub(crate) async fn apply_knowledge_proposal(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<String>,
    Json(body): Json<ApplyKnowledgeBody>,
) -> Response {
    if let Some(response) = write_denied(&user) {
        return response;
    }
    let applied = st
        .storage
        .apply_knowledge_proposal(&id, &body.target_version_id)
        .await;
    let (proposal, version) = match applied {
        Ok(result) => result,
        Err(error) => return error_response(error),
    };

    // Storage is authoritative for compatibility and persistence; only after it
    // commits do we swap the executable definitions used by new runs.
    if let Some(runtime_ir) = version.runtime_ir.as_ref() {
        *st.v2_services.write().await = ir_to_services(runtime_ir);
        *st.v2_effect_defs.write().await = ir_to_runtime(runtime_ir);
    }

    ApiResponse::ok(json!({ "proposal": proposal, "version": version })).into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RollbackKnowledgeBody {
    pub proposal_id: String,
    pub target_version_id: String,
}

pub(crate) async fn rollback_knowledge(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Json(body): Json<RollbackKnowledgeBody>,
) -> Response {
    if let Some(response) = write_denied(&user) {
        return response;
    }
    match st
        .storage
        .propose_knowledge_rollback(body.proposal_id, &body.target_version_id)
        .await
    {
        Ok(record) => ApiResponse::ok(record).into_response(),
        Err(error) => error_response(error),
    }
}

#[derive(Deserialize)]
#[serde(tag = "query", rename_all = "camelCase")]
pub(crate) enum KnowledgeReasonQuery {
    Consistency,
    Classification,
    Satisfiability {
        class: String,
    },
    EntailsSubclass {
        sub: String,
        sup: String,
    },
    EntailsObjectProperty {
        property: String,
        subject: String,
        object: String,
    },
    DataProperty {
        property: String,
        individual: String,
        value: KnowledgeLiteral,
    },
    InstancesOf {
        class: String,
    },
    IsInstanceOf {
        class: String,
        individual: String,
    },
    Realization,
    ExplainSubclass {
        sub: String,
        sup: String,
    },
    ExplainObjectProperty {
        property: String,
        subject: String,
        object: String,
    },
    ExplainDataProperty {
        property: String,
        individual: String,
        value: KnowledgeLiteral,
    },
    ExplainInstance {
        class: String,
        individual: String,
    },
    ExplainUnsatisfiable {
        class: String,
    },
    ExplainInconsistency,
}

fn reasoning_error(error: impl std::fmt::Display) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        ApiResponse::error(wb_code("WB2201"), format!("推理失败: {error}")),
    )
        .into_response()
}

/// One-shot read-only health report for the persisted TBox plus live ABox.
///
/// Evolution gates prevent known-inconsistent versions from being applied, but
/// world facts can change after approval.  Diagnosis therefore rechecks the same
/// snapshot used by reasoning queries and explains any failure in one request.
pub(crate) async fn diagnose_current_knowledge(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
) -> Response {
    if !user.can_read_evaluation() {
        return forbidden("当前角色无权诊断知识本体（需 Operator 及以上）").into_response();
    }
    let version = match st.storage.current_knowledge_version().await {
        Ok(version) => version,
        Err(error) => return error_response(error),
    };
    let Some(schema) = version.map(|version| version.ontology) else {
        return (
            StatusCode::NOT_FOUND,
            ApiResponse::error(404, "知识本体尚未初始化"),
        )
            .into_response();
    };
    let (objects, relations) = match storage_world_snapshot(&st.storage).await {
        Ok(snapshot) => snapshot,
        Err(error) => return reasoning_error(error),
    };
    let cardinality_violations =
        world_snapshot_cardinality_violations(&schema, &objects, &relations);
    let data_cardinality_violations = world_snapshot_data_cardinality_violations(&schema, &objects);
    let knowledge_base = match world_snapshot_to_knowledge(&schema, &objects, &relations) {
        Ok(knowledge_base) => knowledge_base,
        Err(error) => return reasoning_error(error),
    };
    let reasoner = match Reasoner::new(&knowledge_base) {
        Ok(reasoner) => reasoner,
        Err(error) => return reasoning_error(error),
    };

    let consistent = match reasoner.is_consistent() {
        Ok(consistent) => consistent,
        Err(error) => return reasoning_error(error),
    };
    if !consistent {
        if let Err(error) = validate_explanation_limit(&knowledge_base) {
            return reasoning_error(error);
        }
        let inconsistency = match reasoner.explain_inconsistency() {
            Ok(Some(justification)) => justification,
            Ok(None) => {
                return reasoning_error("inconsistent knowledge base has no explanation");
            }
            Err(error) => return reasoning_error(error),
        };
        let result = json!({
            "consistent": false,
            "unsatisfiableClasses": [],
            "unsatisfiable": [],
            "inconsistency": inconsistency,
            "cardinalityViolations": cardinality_violations,
            "dataCardinalityViolations": data_cardinality_violations,
        });
        return ApiResponse::ok(result).into_response();
    }

    if !cardinality_violations.is_empty() || !data_cardinality_violations.is_empty() {
        let result = json!({
            "consistent": true,
            "unsatisfiableClasses": [],
            "unsatisfiable": [],
            "inconsistency": Option::<ReasonedJustification>::None,
            "cardinalityViolations": cardinality_violations,
            "dataCardinalityViolations": data_cardinality_violations,
        });
        return ApiResponse::ok(result).into_response();
    }

    let classification = match reasoner.classify() {
        Ok(classification) => classification,
        Err(error) => return reasoning_error(error),
    };
    if !classification.unsatisfiable_classes().is_empty()
        && let Err(error) = validate_explanation_limit(&knowledge_base)
    {
        return reasoning_error(error);
    }
    let mut unsatisfiable = Vec::new();
    for class in classification.unsatisfiable_classes() {
        let justification = match reasoner.explain_class_unsatisfiable(class) {
            Ok(Some(justification)) => justification,
            Ok(None) => {
                return reasoning_error(format!(
                    "unsatisfiable class `{class}` has no explanation"
                ));
            }
            Err(error) => return reasoning_error(error),
        };
        unsatisfiable.push(json!({
            "class": class,
            "justification": justification,
        }));
    }
    let result = json!({
        "consistent": true,
        "unsatisfiableClasses": classification.unsatisfiable_classes(),
        "unsatisfiable": unsatisfiable,
        "inconsistency": Option::<ReasonedJustification>::None,
        "cardinalityViolations": Vec::<crate::knowledge::RelationCardinalityViolation>::new(),
        "dataCardinalityViolations": Vec::<crate::knowledge::DataCardinalityViolation>::new(),
    });
    ApiResponse::ok(result).into_response()
}

/// 对当前持久化 TBox 与 live World ABox 做只读 DL 推理。
pub(crate) async fn reason_current_knowledge(
    State(st): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Json(request): Json<KnowledgeReasonQuery>,
) -> Response {
    if !user.can_read_evaluation() {
        return forbidden("当前角色无权执行知识推理（需 Operator 及以上）").into_response();
    }
    let version = match st.storage.current_knowledge_version().await {
        Ok(version) => version,
        Err(error) => return error_response(error),
    };
    let Some(schema) = version.map(|version| version.ontology) else {
        return (
            StatusCode::NOT_FOUND,
            ApiResponse::error(404, "知识本体尚未初始化"),
        )
            .into_response();
    };
    let knowledge_base = match storage_world_snapshot_to_knowledge(&schema, &st.storage).await {
        Ok(knowledge_base) => knowledge_base,
        Err(error) => return reasoning_error(error),
    };
    let reasoner = match Reasoner::new(&knowledge_base) {
        Ok(reasoner) => reasoner,
        Err(error) => return reasoning_error(error),
    };

    let result = match request {
        KnowledgeReasonQuery::Consistency => match reasoner.is_consistent() {
            Ok(consistent) => json!({ "consistent": consistent }),
            Err(error) => return reasoning_error(error),
        },
        KnowledgeReasonQuery::Classification => match reasoner.classify() {
            Ok(classified) => {
                let superclasses = classified
                    .classes()
                    .iter()
                    .map(|class| {
                        (
                            class.clone(),
                            classified.superclasses(class).cloned().unwrap_or_default(),
                        )
                    })
                    .collect::<BTreeMap<KnowledgeTerm, _>>();
                let direct_superclasses = classified
                    .classes()
                    .iter()
                    .map(|class| {
                        (
                            class.clone(),
                            classified
                                .direct_superclasses(class)
                                .cloned()
                                .unwrap_or_default(),
                        )
                    })
                    .collect::<BTreeMap<KnowledgeTerm, _>>();
                json!({
                    "classes": classified.classes(),
                    "unsatisfiableClasses": classified.unsatisfiable_classes(),
                    "superclasses": superclasses,
                    "directSuperclasses": direct_superclasses,
                })
            }
            Err(error) => return reasoning_error(error),
        },
        KnowledgeReasonQuery::Satisfiability { class } => {
            match reasoner.is_satisfiable(&KnowledgeTerm::new(class)) {
                Ok(satisfiable) => json!({ "satisfiable": satisfiable }),
                Err(error) => return reasoning_error(error),
            }
        }
        KnowledgeReasonQuery::EntailsSubclass { sub, sup } => {
            match reasoner.entails_subclass(&KnowledgeTerm::new(sub), &KnowledgeTerm::new(sup)) {
                Ok(entailed) => json!({ "entailed": entailed }),
                Err(error) => return reasoning_error(error),
            }
        }
        KnowledgeReasonQuery::EntailsObjectProperty {
            property,
            subject,
            object,
        } => {
            match reasoner.entails_object_property(
                &KnowledgeTerm::new(property),
                &KnowledgeTerm::new(subject),
                &KnowledgeTerm::new(object),
            ) {
                Ok(related) => json!({ "related": related }),
                Err(error) => return reasoning_error(error),
            }
        }
        KnowledgeReasonQuery::DataProperty {
            property,
            individual,
            value,
        } => match reasoner.entails_data_property(
            &KnowledgeTerm::new(property),
            &KnowledgeTerm::new(individual),
            &value,
        ) {
            Ok(entailed) => json!({ "entailed": entailed }),
            Err(error) => return reasoning_error(error),
        },
        KnowledgeReasonQuery::InstancesOf { class } => {
            match reasoner.instances_of(&KnowledgeTerm::new(class)) {
                Ok(instances) => json!({ "instances": instances }),
                Err(error) => return reasoning_error(error),
            }
        }
        KnowledgeReasonQuery::IsInstanceOf { class, individual } => {
            match reasoner
                .is_instance_of(&KnowledgeTerm::new(class), &KnowledgeTerm::new(individual))
            {
                Ok(instance) => json!({ "instance": instance }),
                Err(error) => return reasoning_error(error),
            }
        }
        KnowledgeReasonQuery::ExplainSubclass { sub, sup } => {
            if let Err(error) = validate_explanation_limit(reasoner.ontology()) {
                return reasoning_error(error);
            }
            match reasoner.explain_subclass(&KnowledgeTerm::new(sub), &KnowledgeTerm::new(sup)) {
                Ok(justification) => json!({ "justification": justification }),
                Err(error) => return reasoning_error(error),
            }
        }
        KnowledgeReasonQuery::ExplainUnsatisfiable { class } => {
            if let Err(error) = validate_explanation_limit(reasoner.ontology()) {
                return reasoning_error(error);
            }
            match reasoner.explain_class_unsatisfiable(&KnowledgeTerm::new(class)) {
                Ok(justification) => json!({ "justification": justification }),
                Err(error) => return reasoning_error(error),
            }
        }
        KnowledgeReasonQuery::ExplainObjectProperty {
            property,
            subject,
            object,
        } => {
            if let Err(error) = validate_explanation_limit(reasoner.ontology()) {
                return reasoning_error(error);
            }
            match reasoner.explain_object_property(
                &KnowledgeTerm::new(property),
                &KnowledgeTerm::new(subject),
                &KnowledgeTerm::new(object),
            ) {
                Ok(justification) => json!({ "justification": justification }),
                Err(error) => return reasoning_error(error),
            }
        }
        KnowledgeReasonQuery::ExplainDataProperty {
            property,
            individual,
            value,
        } => {
            if let Err(error) = validate_explanation_limit(reasoner.ontology()) {
                return reasoning_error(error);
            }
            match reasoner.explain_data_property(
                &KnowledgeTerm::new(property),
                &KnowledgeTerm::new(individual),
                &value,
            ) {
                Ok(justification) => json!({ "justification": justification }),
                Err(error) => return reasoning_error(error),
            }
        }
        KnowledgeReasonQuery::ExplainInstance { class, individual } => {
            if let Err(error) = validate_explanation_limit(reasoner.ontology()) {
                return reasoning_error(error);
            }
            match reasoner
                .explain_instance(&KnowledgeTerm::new(class), &KnowledgeTerm::new(individual))
            {
                Ok(justification) => json!({ "justification": justification }),
                Err(error) => return reasoning_error(error),
            }
        }
        KnowledgeReasonQuery::ExplainInconsistency => {
            if let Err(error) = validate_explanation_limit(reasoner.ontology()) {
                return reasoning_error(error);
            }
            match reasoner.explain_inconsistency() {
                Ok(justification) => json!({ "justification": justification }),
                Err(error) => return reasoning_error(error),
            }
        }
        KnowledgeReasonQuery::Realization => match reasoner.realize() {
            Ok(realized) => {
                let entailed_types = realized
                    .individuals()
                    .iter()
                    .map(|individual| {
                        (
                            individual.clone(),
                            realized
                                .entailed_types(individual)
                                .cloned()
                                .unwrap_or_default(),
                        )
                    })
                    .collect::<BTreeMap<KnowledgeTerm, _>>();
                let most_specific_types = realized
                    .individuals()
                    .iter()
                    .map(|individual| {
                        (
                            individual.clone(),
                            realized
                                .most_specific_types(individual)
                                .cloned()
                                .unwrap_or_default(),
                        )
                    })
                    .collect::<BTreeMap<KnowledgeTerm, _>>();
                json!({
                    "individuals": realized.individuals(),
                    "entailedTypes": entailed_types,
                    "mostSpecificTypes": most_specific_types,
                })
            }
            Err(error) => return reasoning_error(error),
        },
    };
    ApiResponse::ok(result).into_response()
}
