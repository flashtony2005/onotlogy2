//! Bridges between the executable runtime ontology and the DL knowledge kernel.
//!
//! ODL remains the authoring/runtime schema.  These pure functions project it,
//! plus current world records, into the append-only knowledge representation;
//! they never mutate storage or infer new facts back into the source.

use crate::abi::ir::OntologyIR;
use crate::abi::validate_object_data;
use crate::knowledge::{
    DataCardinalityViolation, KnowledgeError, Ontology, RelationCardinalityViolation,
    RuntimeObjectFact, RuntimeRelationFact, declared_cardinality_violations,
    declared_data_cardinality_violations, runtime_world_to_knowledge,
};
use crate::storage::{LibSQLStorage, ObjectRecord, RelationRecord};

/// A deterministic failure while projecting runtime records into the DL kernel.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum KnowledgeBridgeError {
    #[error(transparent)]
    Knowledge(#[from] KnowledgeError),
    #[error("object `{object_id}` is incompatible with evolved runtime ontology: {reason}")]
    RuntimeIncompatible { object_id: String, reason: String },
    #[error("storage query failed: {0}")]
    Storage(String),
}

/// Validate an evolved ODL/runtime schema against every current world row.
///
/// This is the closed-world data gate for evolution.  Declared scalar
/// cardinalities and changed ranges are checked against every persisted row.
pub async fn validate_runtime_ir_against_world(
    ir: &OntologyIR,
    storage: &LibSQLStorage,
) -> Result<(), KnowledgeBridgeError> {
    let objects = storage
        .list_all_objects()
        .await
        .map_err(|error| KnowledgeBridgeError::Storage(error.to_string()))?;
    for object in objects {
        let Some(definition) = ir.find(&object.r#type) else {
            continue;
        };
        if let Err(error) = validate_object_data(definition, &object.data) {
            return Err(KnowledgeBridgeError::RuntimeIncompatible {
                object_id: object.id,
                reason: error.to_string(),
            });
        }
    }
    Ok(())
}

/// Read the authoritative current world snapshot.
pub async fn storage_world_snapshot(
    storage: &LibSQLStorage,
) -> Result<(Vec<ObjectRecord>, Vec<RelationRecord>), KnowledgeBridgeError> {
    let objects = storage
        .list_all_objects()
        .await
        .map_err(|error| KnowledgeBridgeError::Storage(error.to_string()))?;
    let relations = storage
        .list_all_relations()
        .await
        .map_err(|error| KnowledgeBridgeError::Storage(error.to_string()))?;
    Ok((objects, relations))
}

/// Read the authoritative current world snapshot and project it to an ABox.
pub async fn storage_world_snapshot_to_knowledge(
    schema: &Ontology,
    storage: &LibSQLStorage,
) -> Result<Ontology, KnowledgeBridgeError> {
    let (objects, relations) = storage_world_snapshot(storage).await?;
    world_snapshot_to_knowledge(schema, &objects, &relations)
}

/// Evaluate closed-world ODL participation bounds against the current snapshot.
pub fn world_snapshot_cardinality_violations(
    schema: &Ontology,
    objects: &[ObjectRecord],
    relations: &[RelationRecord],
) -> Vec<RelationCardinalityViolation> {
    let objects = objects
        .iter()
        .map(|record| RuntimeObjectFact {
            id: record.id.clone(),
            object_type: record.r#type.clone(),
            data: record.data.clone(),
        })
        .collect::<Vec<_>>();
    let relations = relations
        .iter()
        .map(|record| RuntimeRelationFact {
            source: record.source.clone(),
            relation: record.relation.clone(),
            target: record.target.clone(),
        })
        .collect::<Vec<_>>();
    declared_cardinality_violations(schema, &objects, &relations)
}

/// Evaluate closed-world ODL scalar value bounds against the current snapshot.
pub fn world_snapshot_data_cardinality_violations(
    schema: &Ontology,
    objects: &[ObjectRecord],
) -> Vec<DataCardinalityViolation> {
    let objects = objects
        .iter()
        .map(|record| RuntimeObjectFact {
            id: record.id.clone(),
            object_type: record.r#type.clone(),
            data: record.data.clone(),
        })
        .collect::<Vec<_>>();
    declared_data_cardinality_violations(schema, &objects)
}

/// Append live-world ABox assertions to a schema ontology without changing it.
pub fn world_snapshot_to_knowledge(
    schema: &Ontology,
    objects: &[ObjectRecord],
    relations: &[RelationRecord],
) -> Result<Ontology, KnowledgeBridgeError> {
    // Runtime records and pure DL facts use one projection path so datatype
    // normalization, distinct-individual semantics, and conversion failures
    // cannot diverge between facade/API reasoning and governance validation.
    let objects = objects
        .iter()
        .map(|record| RuntimeObjectFact {
            id: record.id.clone(),
            object_type: record.r#type.clone(),
            data: record.data.clone(),
        })
        .collect::<Vec<_>>();
    let relations = relations
        .iter()
        .map(|record| RuntimeRelationFact {
            source: record.source.clone(),
            relation: record.relation.clone(),
            target: record.target.clone(),
        })
        .collect::<Vec<_>>();
    runtime_world_to_knowledge(schema, &objects, &relations).map_err(KnowledgeBridgeError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::ir::{ObjectType, PropertyDef, ScalarType};
    use crate::knowledge::{Axiom, Literal, Reasoner, Term, runtime_ir_to_knowledge};
    use serde_json::json;

    fn ir() -> OntologyIR {
        OntologyIR {
            relations: Vec::new(),
            disjoint_groups: Vec::new(),
            equivalent_groups: Vec::new(),
            relation_hierarchy: Vec::new(),
            disjoint_relation_groups: Vec::new(),
            equivalent_relation_groups: Vec::new(),
            data_property_hierarchy: Vec::new(),
            disjoint_data_property_groups: Vec::new(),
            equivalent_data_property_groups: Vec::new(),
            object_types: vec![ObjectType {
                name: "Customer".into(),
                extends: Vec::new(),
                properties: vec![
                    PropertyDef {
                        name: "name".into(),
                        ty: ScalarType::String,
                        constraints: Vec::new(),
                    },
                    PropertyDef {
                        name: "age".into(),
                        ty: ScalarType::Int,
                        constraints: Vec::new(),
                    },
                    PropertyDef {
                        name: "score".into(),
                        ty: ScalarType::Float,
                        constraints: Vec::new(),
                    },
                    PropertyDef {
                        name: "active".into(),
                        ty: ScalarType::Bool,
                        constraints: Vec::new(),
                    },
                ],
                actions: Vec::new(),
            }],
        }
    }

    #[test]
    fn odl_schema_projects_into_consistent_dl_tbox() {
        let ontology = runtime_ir_to_knowledge(&ir());
        assert_eq!(1 + 4 * 3, ontology.len());
        assert!(Reasoner::new(&ontology).unwrap().is_consistent().unwrap());
    }

    fn data_assertion_value(ontology: &Ontology, property: &str) -> Option<Literal> {
        ontology.axioms().iter().find_map(|axiom| match axiom {
            Axiom::DataPropertyAssertion {
                property: asserted,
                value,
                ..
            } if asserted.as_str() == property => Some(value.clone()),
            _ => None,
        })
    }

    #[test]
    fn world_object_becomes_class_and_scalar_data_assertions() {
        let schema = runtime_ir_to_knowledge(&ir());
        let record = ObjectRecord {
            id: "customer-1".into(),
            r#type: "Customer".into(),
            data: json!({
                "name": "Grace",
                "age": 30,
                "score": 30,
                "active": true,
                "optional": null
            }),
            version: 1,
        };
        let knowledge_base = world_snapshot_to_knowledge(&schema, &[record], &[]).unwrap();
        assert_eq!(
            Some(Literal::Integer(30)),
            data_assertion_value(&knowledge_base, "age")
        );
        // ODL Float accepts JSON integer 30, but its canonical OWL range is double.
        assert_eq!(
            Some(Literal::Double(30.0)),
            data_assertion_value(&knowledge_base, "score")
        );

        let reasoner = Reasoner::new(&knowledge_base).unwrap();
        assert!(reasoner.is_consistent().unwrap());
        let customer = Term::new("Customer");
        let grace = Term::new("customer-1");
        assert!(reasoner.is_instance_of(&customer, &grace).unwrap());
    }

    #[test]
    fn relation_edges_become_object_property_assertions() {
        let schema = runtime_ir_to_knowledge(&ir());
        let customer = ObjectRecord {
            id: "c1".into(),
            r#type: "Customer".into(),
            data: json!({"name": "Grace"}),
            version: 1,
        };
        let order = ObjectRecord {
            id: "o1".into(),
            r#type: "Order".into(),
            data: serde_json::Map::new().into(),
            version: 1,
        };
        let edge = RelationRecord {
            id: "r1".into(),
            source: "c1".into(),
            relation: "placed".into(),
            target: "o1".into(),
            data: None,
        };
        let knowledge_base =
            world_snapshot_to_knowledge(&schema, &[customer, order], &[edge]).unwrap();
        assert!(
            Reasoner::new(&knowledge_base)
                .unwrap()
                .is_consistent()
                .unwrap()
        );
    }

    #[test]
    fn nonscalar_runtime_data_is_rejected_fail_closed() {
        let schema = runtime_ir_to_knowledge(&ir());
        let record = ObjectRecord {
            id: "bad".into(),
            r#type: "Customer".into(),
            data: json!({"tags": ["vip"]}),
            version: 1,
        };
        let error = world_snapshot_to_knowledge(&schema, &[record], &[]).unwrap_err();
        assert!(matches!(
            error,
            KnowledgeBridgeError::Knowledge(KnowledgeError::Invalid(_))
        ));
        assert!(error.to_string().contains("`tags`"));
    }
}

#[cfg(test)]
mod storage_tests {
    use super::*;
    use crate::abi::ir::{ObjectType, PropertyDef, ScalarType};
    use crate::knowledge::{Reasoner, Term, runtime_ir_to_knowledge};
    use crate::storage::{RelationEngine, WorldCommand};
    use serde_json::json;

    #[tokio::test]
    async fn facade_storage_snapshot_projects_live_world() {
        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        storage
            .apply_world(WorldCommand::CreateObject {
                object_type: "Customer".into(),
                id: "c1".into(),
                data: json!({"name": "Grace"}),
                project_id: None,
            })
            .await
            .unwrap();
        storage
            .apply_world(WorldCommand::CreateObject {
                object_type: "Order".into(),
                id: "o1".into(),
                data: serde_json::Map::new().into(),
                project_id: None,
            })
            .await
            .unwrap();
        storage
            .apply_world(WorldCommand::LinkRelation {
                source: "c1".into(),
                edge: "placed".into(),
                target: "o1".into(),
                project_id: None,
            })
            .await
            .unwrap();

        let mut ir = OntologyIR::default();
        ir.object_types.push(ObjectType {
            name: "Customer".into(),
            extends: Vec::new(),
            properties: vec![PropertyDef {
                name: "name".into(),
                ty: ScalarType::String,
                constraints: Vec::new(),
            }],
            actions: Vec::new(),
        });
        let schema = runtime_ir_to_knowledge(&ir);
        let knowledge_base = storage_world_snapshot_to_knowledge(&schema, &storage)
            .await
            .unwrap();
        let reasoner = Reasoner::new(&knowledge_base).unwrap();
        assert!(reasoner.is_consistent().unwrap());
        assert!(
            reasoner
                .is_instance_of(&Term::new("Customer"), &Term::new("c1"))
                .unwrap()
        );
    }

    #[tokio::test]
    async fn world_snapshot_includes_every_relation_beyond_legacy_page_size() {
        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        for (object_type, id) in [("Customer", "c1"), ("Order", "o1")] {
            storage
                .apply_world(WorldCommand::CreateObject {
                    object_type: object_type.into(),
                    id: id.into(),
                    data: json!({}),
                    project_id: None,
                })
                .await
                .unwrap();
        }
        // The old read path silently stopped at 1,000 rows.  A fact just past
        // that boundary must still reach the ABox and therefore the reasoner.
        let last_edge = "edge-1000";
        for index in 0..1001 {
            storage
                .link("c1", format!("edge-{index}").as_str(), "o1", None)
                .await
                .unwrap();
        }

        let relations = storage.list_all_relations().await.unwrap();
        assert_eq!(1001, relations.len());
        let schema = runtime_ir_to_knowledge(&OntologyIR::default());
        let knowledge_base = storage_world_snapshot_to_knowledge(&schema, &storage)
            .await
            .unwrap();
        assert!(
            Reasoner::new(&knowledge_base)
                .unwrap()
                .entails_object_property(&Term::new(last_edge), &Term::new("c1"), &Term::new("o1"))
                .unwrap()
        );
    }
}
