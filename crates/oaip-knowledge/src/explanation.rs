//! Black-box justifications for mature-reasoner conclusions.
//!
//! The upstream tableau reasoner answers *whether* a conclusion follows, but
//! governance also needs to know which asserted axioms are responsible.  This
//! module provides small, irredundant axiom subsets using sound reasoner probes
//! rather than inventing a second inference algorithm.

use crate::error::{KnowledgeError, Result};
use crate::model::{Axiom, ClassExpression, Ontology, Term};
use crate::reasoner::Reasoner;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Transport guard for explanation queries.
///
/// The kernel can explain any ontology the reasoner accepts, but HTTP callers
/// must opt in through this bound so an expensive diagnostic request cannot be
/// used as an accidental denial-of-service vector.
pub const MAX_EXPLANATION_AXIOMS: usize = 512;

/// Number of completed justification queries retained per immutable snapshot.
pub(crate) const EXPLANATION_CACHE_CAPACITY: usize = 128;

/// An irredundant set of asserted axioms supporting one reasoned conclusion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasonedJustification {
    pub axiom_indices: Vec<usize>,
    pub axioms: Vec<Axiom>,
}

/// A tiny exact-LRU cache for expensive black-box justification searches.
#[derive(Debug, Clone)]
pub(crate) struct ExplanationLru {
    entries: VecDeque<(ExplanationKey, Option<ReasonedJustification>)>,
    capacity: usize,
}

impl ExplanationLru {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            capacity: capacity.max(1),
        }
    }

    fn get(&mut self, key: &ExplanationKey) -> Option<Option<ReasonedJustification>> {
        let position = self
            .entries
            .iter()
            .position(|(candidate, _)| candidate == key)?;
        let entry = self.entries.remove(position);
        self.entries
            .push_front(entry.expect("position came from entries"));
        Some(self.entries.front().and_then(|(_, value)| value.clone()))
    }

    fn put(&mut self, key: ExplanationKey, value: Option<ReasonedJustification>) {
        if let Some(position) = self
            .entries
            .iter()
            .position(|(candidate, _)| candidate == &key)
        {
            self.entries.remove(position);
        }
        self.entries.push_front((key, value));
        while self.entries.len() > self.capacity {
            self.entries.pop_back();
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Distinguishes the six bounded explanation query shapes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ExplanationKey {
    Subclass {
        sub: Term,
        sup: Term,
    },
    Instance {
        class: Term,
        individual: Term,
    },
    UnsatisfiableClass {
        class: Term,
    },
    ObjectProperty {
        property: Term,
        subject: Term,
        object: Term,
    },
    DataProperty {
        property: Term,
        individual: Term,
        value_debug: String,
    },
    Inconsistency,
}

impl ReasonedJustification {
    fn new(indices: &[usize], ontology: &Ontology) -> Self {
        Self {
            axiom_indices: indices.to_vec(),
            axioms: indices
                .iter()
                .map(|index| ontology.axioms()[*index].clone())
                .collect(),
        }
    }
}

/// Treat an upstream "unknown named class" probe as a negative entailment.
///
/// During shrinking a premise may be the only declaration of the queried term.
/// In that case the conclusion has become unentailed rather than the explainer
/// having failed. Other reasoner failures remain fail-closed.
fn entailment_probe(probe: crate::error::Result<bool>) -> Result<bool> {
    match probe {
        Ok(entailed) => Ok(entailed),
        Err(KnowledgeError::Reasoner(message))
            if message.starts_with("class IRI not in ontology") =>
        {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

fn term_iri_for_probe(term: &Term) -> String {
    let value = term.as_str();
    if value.contains("://") || value.starts_with("urn:") {
        value.to_owned()
    } else {
        format!("http://example.com/owl#{value}")
    }
}

pub fn validate_explanation_limit(ontology: &Ontology) -> Result<()> {
    if ontology.len() > MAX_EXPLANATION_AXIOMS {
        return Err(KnowledgeError::Invalid(format!(
            "explanation is limited to {MAX_EXPLANATION_AXIOMS} axioms; ontology has {}",
            ontology.len()
        )));
    }
    Ok(())
}

fn shrink<F>(ontology: &Ontology, mut supports: F) -> Result<Vec<usize>>
where
    F: FnMut(&Ontology) -> Result<bool>,
{
    let mut kept = (0..ontology.len()).collect::<Vec<_>>();
    let mut index = 0;
    while index < kept.len() {
        let candidate = kept
            .iter()
            .copied()
            .enumerate()
            .filter(|(position, _)| *position != index)
            .map(|(_, axiom_index)| axiom_index)
            .collect::<Vec<_>>();
        let candidate_ontology = candidate
            .iter()
            .map(|axiom_index| ontology.axioms()[*axiom_index].clone())
            .collect::<Ontology>();
        if !supports(&candidate_ontology)? {
            index += 1;
        } else {
            kept.remove(index);
        }
    }
    Ok(kept)
}

impl Reasoner {
    /// Explain why `sub ⊑ sup` follows, or return `None` when it does not.
    pub fn explain_subclass(
        &self,
        sub: &Term,
        sup: &Term,
    ) -> Result<Option<ReasonedJustification>> {
        let key = ExplanationKey::Subclass {
            sub: sub.clone(),
            sup: sup.clone(),
        };
        self.cached_explanation(key, || {
            if !self.entails_subclass(sub, sup)? {
                return Ok(None);
            }
            let indices = shrink(self.ontology(), |candidate| {
                entailment_probe(Reasoner::new(candidate)?.entails_subclass(sub, sup))
            })?;
            Ok(Some(ReasonedJustification::new(&indices, self.ontology())))
        })
    }

    /// Explain why `individual` belongs to a named class under the open world
    /// semantics, or return `None` when membership is not entailed.
    pub fn explain_instance(
        &self,
        class: &Term,
        individual: &Term,
    ) -> Result<Option<ReasonedJustification>> {
        let key = ExplanationKey::Instance {
            class: class.clone(),
            individual: individual.clone(),
        };
        self.cached_explanation(key, || {
            if !self.is_instance_of(class, individual)? {
                return Ok(None);
            }
            let indices = shrink(self.ontology(), |candidate| {
                entailment_probe(Reasoner::new(candidate)?.is_instance_of(class, individual))
            })?;
            Ok(Some(ReasonedJustification::new(&indices, self.ontology())))
        })
    }

    /// Explain why a named class has no possible instances.
    ///
    /// Satisfiability is reduced to consistency by asserting a fresh probe
    /// individual. The synthetic assertion is used only while shrinking; the
    /// returned justification contains the source axioms that make every model
    /// of the class contradictory.
    pub fn explain_class_unsatisfiable(
        &self,
        class: &Term,
    ) -> Result<Option<ReasonedJustification>> {
        let key = ExplanationKey::UnsatisfiableClass {
            class: class.clone(),
        };
        self.cached_explanation(key, || {
            if self.is_satisfiable(class)? {
                return Ok(None);
            }
            let mut hasher = DefaultHasher::new();
            format!("{:?}", self.ontology().axioms()).hash(&mut hasher);
            let probe_individual = Term::new(format!(
                "urn:oaip:class-explanation:{}:{}",
                hasher.finish(),
                term_iri_for_probe(class)
            ));
            let supports = move |source: &Ontology| -> Result<bool> {
                let mut probe = source.clone();
                probe.push(Axiom::ClassAssertion {
                    class: ClassExpression::Class(class.clone()),
                    individual: probe_individual.clone(),
                });
                Ok(!Reasoner::new(&probe)?.is_consistent()?)
            };
            let indices = shrink(self.ontology(), supports)?;
            Ok(Some(ReasonedJustification::new(&indices, self.ontology())))
        })
    }

    /// Explain why one object-property assertion follows, or return `None`.
    pub fn explain_object_property(
        &self,
        property: &Term,
        subject: &Term,
        object: &Term,
    ) -> Result<Option<ReasonedJustification>> {
        let key = ExplanationKey::ObjectProperty {
            property: property.clone(),
            subject: subject.clone(),
            object: object.clone(),
        };
        self.cached_explanation(key, || {
            if !self.entails_object_property(property, subject, object)? {
                return Ok(None);
            }
            let indices = shrink(self.ontology(), |candidate| {
                Reasoner::new(candidate)?.entails_object_property(property, subject, object)
            })?;
            Ok(Some(ReasonedJustification::new(&indices, self.ontology())))
        })
    }

    /// Explain an entailed typed data fact, including same-individual and sub-property inference.
    pub fn explain_data_property(
        &self,
        property: &Term,
        individual: &Term,
        value: &crate::model::Literal,
    ) -> Result<Option<ReasonedJustification>> {
        let key = ExplanationKey::DataProperty {
            property: property.clone(),
            individual: individual.clone(),
            value_debug: format!("{value:?}"),
        };
        self.cached_explanation(key, || {
            if !self.entails_data_property(property, individual, value)? {
                return Ok(None);
            }
            let indices = shrink(self.ontology(), |candidate| {
                Reasoner::new(candidate)?.entails_data_property(property, individual, value)
            })?;
            Ok(Some(ReasonedJustification::new(&indices, self.ontology())))
        })
    }

    /// Explain an inconsistent knowledge base, or return `None` when it has a model.
    pub fn explain_inconsistency(&self) -> Result<Option<ReasonedJustification>> {
        self.cached_explanation(ExplanationKey::Inconsistency, || {
            if self.is_consistent()? {
                return Ok(None);
            }
            let indices = shrink(self.ontology(), |candidate| {
                Ok(!Reasoner::new(candidate)?.is_consistent()?)
            })?;
            Ok(Some(ReasonedJustification::new(&indices, self.ontology())))
        })
    }

    /// Reuse a completed search without ever caching a failed probe.
    ///
    /// The cache is intentionally private and bounded: it can only contain
    /// results derived from this immutable ontology snapshot.
    fn cached_explanation<F>(
        &self,
        key: ExplanationKey,
        compute: F,
    ) -> Result<Option<ReasonedJustification>>
    where
        F: FnOnce() -> Result<Option<ReasonedJustification>>,
    {
        if let Some(cached) = self
            .explanations
            .lock()
            .expect("explanation cache mutex poisoned")
            .get(&key)
        {
            return Ok(cached);
        }
        let computed = compute()?;
        self.explanations
            .lock()
            .expect("explanation cache mutex poisoned")
            .put(key, computed.clone());
        Ok(computed)
    }

    #[cfg(test)]
    pub(crate) fn explanation_cache_len(&self) -> usize {
        self.explanations
            .lock()
            .expect("explanation cache mutex poisoned")
            .len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ClassExpression;

    fn term(value: &str) -> Term {
        Term::new(value)
    }

    #[test]
    fn bounded_explanation_lru_evicts_the_least_recent_query() {
        let mut cache = ExplanationLru::new(2);
        let first = ExplanationKey::Subclass {
            sub: term("A"),
            sup: term("B"),
        };
        let second = ExplanationKey::Subclass {
            sub: term("B"),
            sup: term("C"),
        };
        let third = ExplanationKey::Subclass {
            sub: term("C"),
            sup: term("D"),
        };

        cache.put(first.clone(), None);
        cache.put(second.clone(), None);
        assert_eq!(cache.get(&first), Some(None), "hit refreshes LRU order");
        cache.put(third, None);

        assert!(cache.get(&first).is_some());
        assert_eq!(cache.get(&second), None);
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn repeated_justifications_reuse_one_bounded_snapshot_entry() {
        let ontology = Ontology::new()
            .with_axiom(Axiom::subclass(named("Puppy"), named("Dog")))
            .with_axiom(Axiom::subclass(named("Dog"), named("Animal")));
        let reasoner = Reasoner::new(&ontology).unwrap();

        let first = reasoner
            .explain_subclass(&term("Puppy"), &term("Animal"))
            .unwrap();
        let second = reasoner
            .explain_subclass(&term("Puppy"), &term("Animal"))
            .unwrap();

        assert_eq!(first, second);
        assert_eq!(1, reasoner.explanation_cache_len());
    }

    fn named(value: &str) -> ClassExpression {
        ClassExpression::class(term(value))
    }

    #[test]
    fn subclass_justification_is_complete_and_irredundant() {
        let ontology = Ontology::new()
            .with_axiom(Axiom::subclass(named("Puppy"), named("Dog")))
            .with_axiom(Axiom::subclass(named("Dog"), named("Animal")))
            .with_axiom(Axiom::subclass(named("Robot"), named("Machine")));
        let reasoner = Reasoner::new(&ontology).unwrap();
        let explanation = reasoner
            .explain_subclass(&term("Puppy"), &term("Animal"))
            .unwrap()
            .expect("transitive subclass should be explainable");
        assert_eq!(vec![0, 1], explanation.axiom_indices);
        assert_eq!(2, explanation.axioms.len());

        assert!(
            reasoner
                .explain_subclass(&term("Robot"), &term("Animal"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn instance_justification_requires_schema_and_assertion() {
        let ontology = Ontology::new()
            .with_axiom(Axiom::subclass(named("Dog"), named("Animal")))
            .with_axiom(Axiom::ClassAssertion {
                class: named("Dog"),
                individual: term("rex"),
            })
            .with_axiom(Axiom::ClassAssertion {
                class: named("Robot"),
                individual: term("bolt"),
            });
        let reasoner = Reasoner::new(&ontology).unwrap();
        let explanation = reasoner
            .explain_instance(&term("Animal"), &term("rex"))
            .unwrap()
            .expect("derived instance should be explainable");
        assert_eq!(vec![0, 1], explanation.axiom_indices);

        assert!(
            reasoner
                .explain_instance(&term("Animal"), &term("bolt"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn object_property_justification_explains_inverse_conclusions() {
        let ontology = Ontology::new()
            .with_axiom(Axiom::InverseObjectProperties {
                first: Term::new("parent"),
                second: Term::new("child"),
            })
            .with_axiom(Axiom::subclass(named("Person"), named("Animal")))
            .with_axiom(Axiom::ObjectPropertyAssertion {
                property: Term::new("parent"),
                subject: term("alice"),
                object: term("bob"),
            });
        let reasoner = Reasoner::new(&ontology).unwrap();
        let explanation = reasoner
            .explain_object_property(&Term::new("child"), &term("bob"), &term("alice"))
            .unwrap()
            .expect("inverse assertion should be explainable");
        assert_eq!(vec![0, 2], explanation.axiom_indices);

        assert!(
            reasoner
                .explain_object_property(&Term::new("child"), &term("alice"), &term("bob"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn data_property_justification_returns_the_assertion() {
        let ontology = Ontology::new()
            .with_axiom(Axiom::subclass(named("Person"), named("Animal")))
            .with_axiom(Axiom::DataPropertyAssertion {
                property: Term::new("age"),
                individual: term("alice"),
                value: crate::model::Literal::Integer(30),
            })
            .with_axiom(Axiom::ClassAssertion {
                class: named("Person"),
                individual: term("alice"),
            });
        let reasoner = Reasoner::new(&ontology).unwrap();
        let explanation = reasoner
            .explain_data_property(
                &Term::new("age"),
                &term("alice"),
                &crate::model::Literal::Integer(30),
            )
            .unwrap()
            .expect("data assertion should be explainable");
        assert_eq!(vec![1], explanation.axiom_indices);

        assert!(
            reasoner
                .explain_data_property(
                    &Term::new("age"),
                    &term("alice"),
                    &crate::model::Literal::Integer(31),
                )
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn inconsistency_justification_contains_conflict_axioms() {
        let ontology = Ontology::new()
            .with_axiom(Axiom::DisjointClasses(vec![named("Dog"), named("Cat")]))
            .with_axiom(Axiom::ClassAssertion {
                class: named("Dog"),
                individual: term("odd"),
            })
            .with_axiom(Axiom::ClassAssertion {
                class: named("Cat"),
                individual: term("odd"),
            })
            .with_axiom(Axiom::subclass(named("Robot"), named("Machine")));
        let reasoner = Reasoner::new(&ontology).unwrap();
        let explanation = reasoner
            .explain_inconsistency()
            .unwrap()
            .expect("inconsistency should be explainable");
        assert_eq!(vec![0, 1, 2], explanation.axiom_indices);
        assert!(!reasoner.is_consistent().unwrap());
    }

    #[test]
    fn asserted_data_conflict_has_an_inconsistency_justification() {
        let ontology = Ontology::new()
            .with_axiom(Axiom::DataPropertyAssertion {
                property: term("age"),
                individual: term("alice"),
                value: crate::model::Literal::Integer(30),
            })
            .with_axiom(Axiom::NegativeDataPropertyAssertion {
                property: term("age"),
                individual: term("alice"),
                value: crate::model::Literal::Integer(30),
            });
        let reasoner = Reasoner::new(&ontology).unwrap();
        assert!(!reasoner.is_consistent().unwrap());

        let explanation = reasoner
            .explain_inconsistency()
            .unwrap()
            .expect("data conflict should be explainable");
        assert_eq!(vec![0, 1], explanation.axiom_indices);
    }

    #[test]
    fn unsatisfiable_class_justification_excludes_the_synthetic_probe() {
        let ontology = Ontology::new()
            .with_axiom(Axiom::DisjointClasses(vec![named("Dog"), named("Cat")]))
            .with_axiom(Axiom::subclass(named("Dog"), named("Cat")))
            .with_axiom(Axiom::subclass(named("Cat"), named("Animal")));
        let reasoner = Reasoner::new(&ontology).unwrap();
        assert!(!reasoner.is_satisfiable(&term("Dog")).unwrap());

        let explanation = reasoner
            .explain_class_unsatisfiable(&term("Dog"))
            .unwrap()
            .expect("unsatisfiability should be explainable");
        assert_eq!(vec![0, 1], explanation.axiom_indices);
        assert!(
            !explanation
                .axiom_indices
                .iter()
                .any(|index| *index >= ontology.len())
        );
    }

    #[test]
    fn property_chain_entailment_is_explainable() {
        let has_parent = Term::new("hasParent");
        let has_grandparent = Term::new("hasGrandparent");
        let ontology = Ontology::new()
            .with_axiom(Axiom::SubObjectPropertyChainOf {
                properties: vec![has_parent.clone(), has_parent.clone()],
                super_property: has_grandparent.clone(),
            })
            .with_axiom(Axiom::ObjectPropertyAssertion {
                property: has_parent.clone(),
                subject: term("alice"),
                object: term("bob"),
            })
            .with_axiom(Axiom::ObjectPropertyAssertion {
                property: has_parent.clone(),
                subject: term("bob"),
                object: term("carol"),
            });

        let reasoner = Reasoner::new(&ontology).unwrap();
        assert!(
            reasoner
                .entails_object_property(&has_grandparent, &term("alice"), &term("carol"))
                .unwrap()
        );

        let explanation = reasoner
            .explain_object_property(&has_grandparent, &term("alice"), &term("carol"))
            .unwrap()
            .expect("chain entailment should be explainable");
        assert!(
            explanation.axiom_indices.contains(&0),
            "the chain axiom must be part of the justification"
        );
    }
}
