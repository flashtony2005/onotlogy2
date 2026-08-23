//! OpenAIP knowledge-representation kernel.
pub mod data;
pub mod error;
pub mod evolution;
pub mod explanation;
pub mod fact;
pub mod model;
pub mod reasoner;
pub mod runtime;
pub mod tbox;

pub use data::validate_data_consistency;
pub use data::{DataConsistencyCacheStats, data_consistency_cache_stats};
pub use error::{KnowledgeError, Result};
pub use evolution::{
    OntologyChange, OntologyImpact, OntologyProposal, OntologyReview, OntologyReviewDecision,
    apply as apply_ontology_proposal, validate as validate_ontology_proposal,
};
pub use explanation::{MAX_EXPLANATION_AXIOMS, ReasonedJustification, validate_explanation_limit};
pub use fact::{Fact, Justification, Rule, Truth, Violation, ViolationKind};
pub use model::{
    Axiom, ClassExpression, DataFacet, DataRange, Datatype, FacetValue, Literal, Ontology, Term,
};
pub use reasoner::{ReasonedClassification, ReasonedRealization, Reasoner};
pub use runtime::{
    DataCardinalityViolation, RelationCardinalityViolation, RuntimeObjectFact, RuntimeRelationFact,
    canonical_runtime_ontology, declared_cardinality_violations,
    declared_data_cardinality_violations, relation_endpoints, runtime_ir_to_knowledge,
    runtime_world_to_knowledge, strip_runtime_ir_from_knowledge, validate_canonical_ontology,
    validate_canonical_ontology_with_world, validate_runtime_ir,
};
pub use tbox::{Classification, TBox};

#[cfg(feature = "test-util")]
pub use data::{data_consistency_thread_stats, reset_data_consistency_thread_stats};
