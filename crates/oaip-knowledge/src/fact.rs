use crate::model::Term;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// A ground conclusion produced by ABox materialization.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Fact {
    ClassAssertion {
        class: Term,
        individual: Term,
    },
    PropertyAssertion {
        property: Term,
        subject: Term,
        object: Term,
    },
}

impl Fact {
    pub fn class(class: impl Into<Term>, individual: impl Into<Term>) -> Self {
        Self::ClassAssertion {
            class: class.into(),
            individual: individual.into(),
        }
    }

    pub fn property(
        property: impl Into<Term>,
        subject: impl Into<Term>,
        object: impl Into<Term>,
    ) -> Self {
        Self::PropertyAssertion {
            property: property.into(),
            subject: subject.into(),
            object: object.into(),
        }
    }

    pub fn terms(&self) -> BTreeSet<Term> {
        match self {
            Self::ClassAssertion { individual, .. } => BTreeSet::from([individual.clone()]),
            Self::PropertyAssertion {
                subject, object, ..
            } => BTreeSet::from([subject.clone(), object.clone()]),
        }
    }
}

/// Deterministic rule label used by explanations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Rule {
    AssertedAxiom,
    SameIndividualExpansion,
    SubClassExpansion,
    UniversalClassExpansion,
    SubPropertyExpansion,
    InversePropertyExpansion,
    SymmetricPropertyExpansion,
    TransitivePropertyExpansion,
    PropertyDomainExpansion,
    PropertyRangeExpansion,
}

/// One justified derivation.  Premises refer to already accepted facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Justification {
    pub rule: Rule,
    pub axiom_indices: BTreeSet<usize>,
    pub premises: Vec<Fact>,
}

impl Justification {
    pub fn asserted(axiom_index: usize) -> Self {
        Self {
            rule: Rule::AssertedAxiom,
            axiom_indices: BTreeSet::from([axiom_index]),
            premises: Vec::new(),
        }
    }

    pub fn derived(rule: Rule, axiom_indices: impl IntoIterator<Item = usize>) -> Self {
        Self {
            rule,
            axiom_indices: axiom_indices.into_iter().collect(),
            premises: Vec::new(),
        }
    }

    pub fn with_premises(mut self, premises: impl IntoIterator<Item = Fact>) -> Self {
        self.premises = premises.into_iter().collect();
        self
    }
}

/// Three-valued entailment under the open-world assumption.
///
/// `Unknown` is central to sound KR: absent evidence is not evidence of absence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Truth {
    True,
    False,
    Unknown,
}

impl Truth {
    pub fn is_true(self) -> bool {
        matches!(self, Self::True)
    }

    pub fn is_false(self) -> bool {
        matches!(self, Self::False)
    }

    #[must_use]
    pub fn and(self, other: Self) -> Self {
        use Truth::{False, True, Unknown};
        match (self, other) {
            (True, True) => True,
            (False, _) | (_, False) => False,
            _ => Unknown,
        }
    }

    #[must_use]
    pub fn or(self, other: Self) -> Self {
        use Truth::{False, True, Unknown};
        match (self, other) {
            (True, _) | (_, True) => True,
            (False, False) => False,
            _ => Unknown,
        }
    }
}

impl std::ops::Not for Truth {
    type Output = Self;

    fn not(self) -> Self::Output {
        match self {
            Self::True => Self::False,
            Self::False => Self::True,
            Self::Unknown => Self::Unknown,
        }
    }
}

/// An explicit logical contradiction detected in the asserted+derived closure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Violation {
    pub kind: ViolationKind,
    pub subject: Term,
    pub message: String,
    pub evidence: BTreeSet<Fact>,
    pub axiom_indices: BTreeSet<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViolationKind {
    DifferentIndividuals,
    DisjointClasses,
    ComplementConflict,
    EmptyClassInstance,
    CardinalityExceeded,
    ComplexExpressionFalse,
}
