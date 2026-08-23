//! Semantic validation for immutable ontology versions.
//!
//! An [`OntologyProposal`] is an explicit change set.  Applying it creates a
//! new in-memory ontology; the source version remains unchanged.  Validation
//! compares mature-reasoner conclusions before and after the change so a
//! proposal cannot silently introduce inconsistency or hidden unsatisfiable
//! classes.

use crate::ReasonedJustification;
use crate::error::{KnowledgeError, Result};
use crate::model::{Axiom, Ontology, Term};
use crate::reasoner::Reasoner;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// One explicit change between two ontology versions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OntologyChange {
    Add(Axiom),
    Remove(Axiom),
    /// Explicit whole-version replacement, used only by reviewed rollback.
    Replace(Ontology),
}

/// A named, auditable ontology change set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OntologyProposal {
    id: String,
    changes: Vec<OntologyChange>,
}

impl OntologyProposal {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            changes: Vec::new(),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn changes(&self) -> &[OntologyChange] {
        &self.changes
    }

    pub fn with_change(mut self, change: OntologyChange) -> Self {
        self.changes.push(change);
        self
    }

    /// Materialize the target version without mutating the source version.
    pub fn target(&self, source: &Ontology) -> Result<Ontology> {
        let mut target = source.clone();
        for change in &self.changes {
            match change {
                OntologyChange::Add(axiom) => {
                    target.push(axiom.clone());
                }
                OntologyChange::Remove(expected) => {
                    let before = target.len();
                    target = target
                        .axioms()
                        .iter()
                        .filter(|actual| *actual != expected)
                        .cloned()
                        .collect();
                    if target.len() == before {
                        return Err(KnowledgeError::Invalid(format!(
                            "proposal {} removes an axiom that is absent from the source",
                            self.id
                        )));
                    }
                }
                OntologyChange::Replace(next) => {
                    target = next.clone();
                }
            }
        }
        Ok(target)
    }
}

/// Reasoner-derived differences introduced by a proposal.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OntologyImpact {
    pub current_consistent: bool,
    pub target_consistent: bool,
    pub newly_unsatisfiable: BTreeSet<Term>,
    /// Source axioms that make each newly empty concept impossible.
    pub unsatisfiable_reasons: BTreeMap<Term, ReasonedJustification>,
    pub resolved_unsatisfiable: BTreeSet<Term>,
    pub added_subsumptions: BTreeSet<(Term, Term)>,
    pub removed_subsumptions: BTreeSet<(Term, Term)>,
}

impl OntologyImpact {
    /// A proposal is semantically admissible when it preserves global
    /// consistency and does not create an empty concept.
    pub fn is_admissible(&self) -> bool {
        self.target_consistent && self.newly_unsatisfiable.is_empty()
    }

    /// Whether removing prior conclusions requires explicit human review even
    /// when the target remains admissible.
    pub fn is_lossy(&self) -> bool {
        !self.resolved_unsatisfiable.is_empty() || !self.removed_subsumptions.is_empty()
    }
}

/// Human review decision. A pending review never authorizes application.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum OntologyReviewDecision {
    #[default]
    Pending,
    Approved,
    Rejected,
}

impl OntologyReviewDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
        }
    }
}

/// The result of comparing the source and target versions, including the
/// persistent human-review decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OntologyReview {
    proposal_id: String,
    impact: OntologyImpact,
    decision: OntologyReviewDecision,
    reviewer: Option<String>,
    reason: Option<String>,
}

impl OntologyReview {
    pub fn proposal_id(&self) -> &str {
        &self.proposal_id
    }

    pub fn impact(&self) -> &OntologyImpact {
        &self.impact
    }

    pub fn decision(&self) -> OntologyReviewDecision {
        self.decision
    }

    pub fn reviewer(&self) -> Option<&str> {
        self.reviewer.as_deref()
    }

    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    pub fn is_admissible(&self) -> bool {
        self.impact.is_admissible()
    }

    pub fn is_approved(&self) -> bool {
        self.decision == OntologyReviewDecision::Approved
    }

    pub fn is_lossy(&self) -> bool {
        self.impact.is_lossy()
    }

    /// Explicit human approval is mandatory; validation never grants it.
    pub fn approve(&mut self, reviewer: impl Into<String>) -> Result<()> {
        if self.decision != OntologyReviewDecision::Pending {
            return Err(KnowledgeError::Invalid(format!(
                "proposal {} has already been reviewed",
                self.proposal_id
            )));
        }
        if !self.is_admissible() {
            return Err(KnowledgeError::Invalid(format!(
                "proposal {} is not semantically admissible",
                self.proposal_id
            )));
        }
        let reviewer = reviewer.into();
        if reviewer.trim().is_empty() {
            return Err(KnowledgeError::Invalid(
                "reviewer must not be empty".to_owned(),
            ));
        }
        self.decision = OntologyReviewDecision::Approved;
        self.reviewer = Some(reviewer);
        self.reason = None;
        Ok(())
    }

    /// Persistently reject an otherwise syntactically valid proposal.
    pub fn reject(&mut self, reviewer: impl Into<String>, reason: impl Into<String>) -> Result<()> {
        if self.decision != OntologyReviewDecision::Pending {
            return Err(KnowledgeError::Invalid(format!(
                "proposal {} has already been reviewed",
                self.proposal_id
            )));
        }
        let reviewer = reviewer.into();
        let reason = reason.into();
        if reviewer.trim().is_empty() || reason.trim().is_empty() {
            return Err(KnowledgeError::Invalid(
                "rejected proposals require a reviewer and reason".to_owned(),
            ));
        }
        self.decision = OntologyReviewDecision::Rejected;
        self.reviewer = Some(reviewer);
        self.reason = Some(reason);
        Ok(())
    }
}

/// Validate a proposal against its source ontology using full DL conclusions.
pub fn validate(proposal: &OntologyProposal, source: &Ontology) -> Result<OntologyReview> {
    let current = Reasoner::new(source)?;
    let target_ontology = proposal.target(source)?;
    let target = Reasoner::new(&target_ontology)?;
    let current_classified = current.classify()?;
    let target_classified = target.classify()?;

    let mut all_classes = current_classified
        .classes()
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    all_classes.extend(target_classified.classes().iter().cloned());

    let subsumptions = |classified: &crate::ReasonedClassification| {
        let mut out = BTreeSet::new();
        for left in &all_classes {
            for right in &all_classes {
                if classified.entails_subclass(left, right) {
                    out.insert((left.clone(), right.clone()));
                }
            }
        }
        out
    };
    let current_subsumptions = subsumptions(&current_classified);
    let target_subsumptions = subsumptions(&target_classified);

    let current_unsat = current_classified.unsatisfiable_classes().clone();
    let target_unsat = target_classified.unsatisfiable_classes().clone();
    let newly_unsatisfiable = target_unsat
        .difference(&current_unsat)
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut unsatisfiable_reasons = BTreeMap::new();
    for class in &newly_unsatisfiable {
        if let Some(reason) = target.explain_class_unsatisfiable(class)? {
            unsatisfiable_reasons.insert(class.clone(), reason);
        }
    }

    Ok(OntologyReview {
        proposal_id: proposal.id().to_owned(),
        impact: OntologyImpact {
            current_consistent: current.is_consistent()?,
            target_consistent: target.is_consistent()?,
            newly_unsatisfiable,
            unsatisfiable_reasons,
            resolved_unsatisfiable: current_unsat.difference(&target_unsat).cloned().collect(),
            added_subsumptions: target_subsumptions
                .difference(&current_subsumptions)
                .cloned()
                .collect(),
            removed_subsumptions: current_subsumptions
                .difference(&target_subsumptions)
                .cloned()
                .collect(),
        },
        decision: OntologyReviewDecision::Pending,
        reviewer: None,
        reason: None,
    })
}

/// Apply an already validated and explicitly approved proposal.
pub fn apply(
    proposal: &OntologyProposal,
    review: &OntologyReview,
    source: &Ontology,
) -> Result<Ontology> {
    if review.proposal_id() != proposal.id() {
        return Err(KnowledgeError::Invalid(
            "review belongs to another proposal".to_owned(),
        ));
    }
    if !review.is_approved() || review.reviewer().is_none() {
        return Err(KnowledgeError::Invalid(format!(
            "proposal {} has not been approved by a named reviewer",
            proposal.id()
        )));
    }
    proposal.target(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ClassExpression;

    fn class(name: &str) -> ClassExpression {
        ClassExpression::class(format!("http://example.com/owl#{name}"))
    }

    fn term(name: &str) -> Term {
        Term::new(format!("http://example.com/owl#{name}"))
    }

    fn source() -> Ontology {
        Ontology::new().with_axiom(Axiom::subclass(class("Dog"), class("Animal")))
    }

    #[test]
    fn safe_addition_reports_derived_impact_and_requires_approval() {
        let proposal = OntologyProposal::new("add-puppy").with_change(OntologyChange::Add(
            Axiom::subclass(class("Puppy"), class("Dog")),
        ));
        let review = validate(&proposal, &source()).unwrap();

        assert!(review.is_admissible());
        assert!(!review.is_approved());
        assert!(
            review
                .impact()
                .added_subsumptions
                .contains(&(term("Puppy"), term("Animal")))
        );

        let applied = apply(&proposal, &review, &source());
        assert!(
            applied.is_err(),
            "validation alone must not authorize evolution"
        );

        let mut approved = review;
        approved.approve("alice").unwrap();
        let target = apply(&proposal, &approved, &source()).unwrap();
        let original = source();
        assert_eq!(original.len() + 1, target.len());
        assert_eq!(1, original.len(), "the original version remains immutable");
    }

    #[test]
    fn inconsistent_target_is_rejected_before_approval() {
        let proposal =
            OntologyProposal::new("make-empty").with_change(OntologyChange::Add(Axiom::subclass(
                class("Empty"),
                ClassExpression::intersection([
                    class("Animal"),
                    ClassExpression::complement(class("Animal")),
                ]),
            )));
        let mut review = validate(&proposal, &source()).unwrap();
        assert!(!review.is_admissible());
        assert_eq!(
            review.impact().newly_unsatisfiable,
            BTreeSet::from([term("Empty")])
        );
        let reason = review
            .impact()
            .unsatisfiable_reasons
            .get(&term("Empty"))
            .expect("newly empty concepts must carry an explanation");
        assert!(!reason.axioms.is_empty());
        assert!(review.approve("alice").is_err());
        assert!(apply(&proposal, &review, &source()).is_err());
    }

    #[test]
    fn lossy_removal_is_flagged_for_human_review() {
        let removed = Axiom::subclass(class("Dog"), class("Animal"));
        let proposal =
            OntologyProposal::new("remove-animal").with_change(OntologyChange::Remove(removed));
        let review = validate(&proposal, &source()).unwrap();

        assert!(review.is_admissible());
        assert!(review.is_lossy());
        assert!(
            review
                .impact()
                .removed_subsumptions
                .contains(&(term("Dog"), term("Animal")))
        );

        let mut approved = review;
        approved.approve("alice").unwrap();
        let target = apply(&proposal, &approved, &source()).unwrap();
        assert_eq!(0, target.len());
    }

    #[test]
    fn removing_missing_axiom_fails_closed() {
        let proposal = OntologyProposal::new("bad-remove").with_change(OntologyChange::Remove(
            Axiom::subclass(class("Cat"), class("Animal")),
        ));
        assert!(proposal.target(&source()).is_err());
    }
}
