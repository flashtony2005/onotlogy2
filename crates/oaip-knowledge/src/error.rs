use std::result;

/// Errors returned while indexing an asserted ontology.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum KnowledgeError {
    /// The kernel refuses to silently ignore syntax it cannot reason about.
    #[error("axiom {index} uses reasoning beyond the current kernel: {reason}")]
    UnsupportedAxiom { index: usize, reason: String },
    #[error("invalid knowledge base: {0}")]
    Invalid(String),
    #[error("OWL DL reasoner failed: {0}")]
    Reasoner(String),
}

pub type Result<T> = result::Result<T, KnowledgeError>;
