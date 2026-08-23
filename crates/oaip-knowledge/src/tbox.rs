//! Terminological-box indexing and classification.
//!
//! The index normalizes equivalence into two directions, computes the named
//! concept lattice, and reports concepts that provably have no instances.  It
//! never mutates the source ontology and keeps axiom indexes so conclusions can
//! later be traced back to their assertion.

use crate::error::{KnowledgeError, Result};
use crate::model::{Axiom, ClassExpression, Ontology, Term};
use std::collections::{BTreeMap, BTreeSet};

/// Indexed TBox derived from an [`Ontology`].
#[derive(Debug, Clone, Default)]
pub struct TBox {
    positive_edges: BTreeMap<Term, BTreeSet<Term>>,
    negative_edges: BTreeMap<Term, BTreeSet<Term>>,
    bottom_classes: BTreeSet<Term>,
    edge_axioms: BTreeMap<(Term, Term), usize>,
    negative_edge_axioms: BTreeMap<(Term, Term), usize>,
    disjoint_groups: Vec<(BTreeSet<Term>, usize)>,
    domains: Vec<(Term, ClassExpression, usize)>,
    ranges: Vec<(Term, ClassExpression, usize)>,
    property_edges: BTreeMap<Term, BTreeSet<Term>>,
    inverse_pairs: BTreeSet<(Term, Term)>,
    symmetric_properties: BTreeSet<Term>,
    transitive_properties: BTreeSet<Term>,
    functional_properties: BTreeSet<Term>,
    inverse_functional_properties: BTreeSet<Term>,
    reflexive_properties: BTreeSet<Term>,
    irreflexive_properties: BTreeSet<Term>,
    asymmetric_properties: BTreeSet<Term>,
    complex_subsumptions: Vec<(ClassExpression, ClassExpression, usize)>,
    unsupported_axioms: BTreeSet<usize>,
}

impl TBox {
    pub fn build(ontology: &Ontology) -> Self {
        let mut tbox = Self::default();

        for (index, axiom) in ontology.axioms().iter().enumerate() {
            match axiom {
                Axiom::SubClassOf { sub, sup } => {
                    tbox.add_subsumption(index, sub.clone(), sup.clone());
                }
                Axiom::EquivalentClasses(classes) => {
                    for left in 0..classes.len() {
                        for right in 0..classes.len() {
                            if left != right {
                                tbox.add_subsumption(
                                    index,
                                    classes[left].clone(),
                                    classes[right].clone(),
                                );
                            }
                        }
                    }
                }
                Axiom::DisjointClasses(classes) => {
                    let terms = classes.iter().filter_map(|c| match c {
                        ClassExpression::Class(term) => Some(term.clone()),
                        _ => None,
                    });
                    let group: BTreeSet<Term> = terms.collect();
                    if group.len() != classes.len() || classes.len() < 2 {
                        tbox.unsupported_axioms.insert(index);
                    } else {
                        tbox.disjoint_groups.push((group, index));
                    }
                }
                Axiom::ObjectPropertyDomain { property, class } => {
                    tbox.domains.push((property.clone(), class.clone(), index))
                }
                Axiom::ObjectPropertyRange { property, class } => {
                    tbox.ranges.push((property.clone(), class.clone(), index))
                }
                Axiom::SubObjectPropertyOf { sub, sup } => {
                    tbox.property_edges
                        .entry(sub.clone())
                        .or_default()
                        .insert(sup.clone());
                }
                Axiom::TransitiveObjectProperty(property) => {
                    tbox.transitive_properties.insert(property.clone());
                }
                Axiom::SymmetricObjectProperty(property) => {
                    tbox.symmetric_properties.insert(property.clone());
                }
                Axiom::FunctionalObjectProperty(property) => {
                    tbox.functional_properties.insert(property.clone());
                }
                Axiom::InverseFunctionalObjectProperty(property) => {
                    tbox.inverse_functional_properties.insert(property.clone());
                }
                Axiom::ReflexiveObjectProperty(property) => {
                    tbox.reflexive_properties.insert(property.clone());
                }
                Axiom::IrreflexiveObjectProperty(property) => {
                    tbox.irreflexive_properties.insert(property.clone());
                }
                Axiom::AsymmetricObjectProperty(property) => {
                    tbox.asymmetric_properties.insert(property.clone());
                }
                Axiom::InverseObjectProperties { first, second } => {
                    tbox.inverse_pairs.insert((first.clone(), second.clone()));
                    tbox.inverse_pairs.insert((second.clone(), first.clone()));
                }
                Axiom::ClassAssertion { .. }
                | Axiom::ObjectPropertyAssertion { .. }
                | Axiom::NegativeObjectPropertyAssertion { .. }
                | Axiom::SameIndividuals(_)
                | Axiom::DifferentIndividuals(_)
                | Axiom::SubDataPropertyOf { .. }
                | Axiom::DisjointDataProperties(_)
                | Axiom::DataPropertyDomain { .. }
                | Axiom::DataPropertyRange { .. }
                | Axiom::DataPropertyOneOf { .. }
                | Axiom::DataPropertyRangeRestriction { .. }
                | Axiom::FunctionalDataProperty(_)
                | Axiom::DataPropertyAssertion { .. }
                | Axiom::NegativeDataPropertyAssertion { .. } => {}
            }
        }

        tbox
    }

    fn add_subsumption(&mut self, index: usize, sub: ClassExpression, sup: ClassExpression) {
        let Some(sub_term) = atomic_class(&sub) else {
            self.complex_subsumptions.push((sub, sup, index));
            self.unsupported_axioms.insert(index);
            return;
        };

        match sup {
            ClassExpression::Class(sup_term) => {
                self.positive_edges
                    .entry(sub_term.clone())
                    .or_default()
                    .insert(sup_term.clone());
                self.edge_axioms.insert((sub_term, sup_term), index);
            }
            ClassExpression::Complement(inner) if matches!(*inner, ClassExpression::Class(_)) => {
                let ClassExpression::Class(forbidden) = *inner else {
                    unreachable!("checked above");
                };
                self.negative_edges
                    .entry(sub_term.clone())
                    .or_default()
                    .insert(forbidden.clone());
                self.negative_edge_axioms
                    .insert((sub_term, forbidden), index);
            }
            ClassExpression::Nothing => {
                self.bottom_classes.insert(sub_term);
            }
            sup => {
                self.complex_subsumptions
                    .push((ClassExpression::Class(sub_term), sup, index));
                self.unsupported_axioms.insert(index);
            }
        }
    }

    /// Axioms retained but not used by the current sound forward-chaining rules.
    pub fn unsupported_axioms(&self) -> &BTreeSet<usize> {
        &self.unsupported_axioms
    }

    /// Named superclass closure including each class itself.
    pub fn ancestors(&self, class: &Term) -> BTreeSet<Term> {
        let mut seen = BTreeSet::from([class.clone()]);
        let mut queue = vec![class.clone()];
        while let Some(current) = queue.pop() {
            if let Some(next) = self.positive_edges.get(&current) {
                for parent in next {
                    if seen.insert(parent.clone()) {
                        queue.push(parent.clone());
                    }
                }
            }
        }
        seen
    }

    /// All named descendants (including `term`) for a named class.
    pub fn descendants(&self, class: &Term) -> BTreeSet<Term> {
        let mut reverse: BTreeMap<&Term, BTreeSet<&Term>> = BTreeMap::new();
        for (sub, sups) in &self.positive_edges {
            for sup in sups {
                reverse.entry(sup).or_default().insert(sub);
            }
        }

        let mut seen = BTreeSet::from([class.clone()]);
        let mut queue = vec![class.clone()];
        while let Some(current) = queue.pop() {
            if let Some(children) = reverse.get(&current) {
                for child in children {
                    if seen.insert((*child).clone()) {
                        queue.push((*child).clone());
                    }
                }
            }
        }
        seen.into_iter().collect()
    }

    pub fn direct_superclasses(&self, class: &Term) -> BTreeSet<Term> {
        self.positive_edges.get(class).cloned().unwrap_or_default()
    }

    pub fn direct_subclasses(&self, class: &Term) -> BTreeSet<Term> {
        self.descendants(class)
            .into_iter()
            .filter(|candidate| candidate != class && !self.ancestors(candidate).contains(class))
            .collect()
    }

    /// True when `sub ⊑ sup` follows from indexed named-class inclusion axioms.
    pub fn entails_subclass(&self, sub: &Term, sup: &Term) -> bool {
        self.ancestors(sub).contains(sup)
    }

    /// Concepts that cannot have a member without violating the TBox.
    ///
    /// This detects explicit bottom constraints, contradictions through negation,
    /// and disjointness conflicts anywhere on a subsumption path.
    pub fn classify(self) -> Classification {
        let mut all_terms = self.positive_edges.keys().cloned().collect::<BTreeSet<_>>();
        for parents in self.positive_edges.values() {
            all_terms.extend(parents.iter().cloned());
        }
        for children in self.descendant_map() {
            all_terms.extend(children.1);
        }
        for (group, _) in &self.disjoint_groups {
            all_terms.extend(group.iter().cloned());
        }

        let mut unsatisfiable = BTreeSet::new();
        let mut ancestors = BTreeMap::new();
        for term in all_terms {
            let closure = self.ancestors(&term);
            let has_bottom = closure.intersection(&self.bottom_classes).next().is_some();
            let has_negation = self
                .negative_reachable_set(&closure)
                .iter()
                .any(|forbidden| closure.contains(forbidden));
            let disjoint_conflict = self
                .disjoint_groups
                .iter()
                .any(|(group, _)| closure.intersection(group).count() > 1);

            if has_bottom || has_negation || disjoint_conflict {
                unsatisfiable.insert(term.clone());
            }
            ancestors.insert(term, closure);
        }

        Classification {
            ancestors,
            unsatisfiable,
        }
    }

    fn descendant_map(&self) -> BTreeMap<Term, BTreeSet<Term>> {
        let mut out: BTreeMap<Term, BTreeSet<Term>> = BTreeMap::new();
        for (sub, sups) in &self.positive_edges {
            for sup in sups {
                out.entry(sup.clone()).or_default().insert(sub.clone());
            }
        }
        out
    }

    fn negative_reachable_set(&self, from: &BTreeSet<Term>) -> BTreeSet<Term> {
        let mut out = BTreeSet::new();
        for term in from {
            if let Some(forbidden) = self.negative_edges.get(term) {
                out.extend(forbidden.iter().cloned());
            }
        }
        out
    }

    /// Transitive closure of object-subproperty edges (`p ⊑ q`).
    pub fn expanded_properties(&self, property: &Term) -> BTreeSet<Term> {
        let mut seen = BTreeSet::from([property.clone()]);
        let mut queue = vec![property.clone()];
        while let Some(current) = queue.pop() {
            if let Some(parents) = self.property_edges.get(&current) {
                for parent in parents {
                    if seen.insert(parent.clone()) {
                        queue.push(parent.clone());
                    }
                }
            }
        }
        seen
    }

    /// Validate that every retained axiom is actually supported by this layer.
    pub fn validate_supported(&self) -> Result<()> {
        let first = self.unsupported_axioms.iter().next();
        if let Some(index) = first {
            return Err(KnowledgeError::UnsupportedAxiom {
                index: *index,
                reason: "non-atomic subsumption or non-named disjoint operand".to_owned(),
            });
        }
        Ok(())
    }
}

/// Result of computing the full named-class hierarchy.
#[derive(Debug, Clone, Default)]
pub struct Classification {
    ancestors: BTreeMap<Term, BTreeSet<Term>>,
    unsatisfiable: BTreeSet<Term>,
}

impl Classification {
    pub fn ancestors(&self, class: &Term) -> Option<&BTreeSet<Term>> {
        self.ancestors.get(class)
    }

    pub fn is_satisfiable(&self, class: &Term) -> bool {
        !self.unsatisfiable.contains(class)
    }

    pub fn unsatisfiable_classes(&self) -> &BTreeSet<Term> {
        &self.unsatisfiable
    }

    /// The whole TBox is globally consistent iff it has no empty named concept.
    pub fn is_consistent(&self) -> bool {
        self.unsatisfiable.is_empty()
    }
}

fn atomic_class(expression: &ClassExpression) -> Option<Term> {
    match expression {
        ClassExpression::Class(term) => Some(term.clone()),
        _ => None,
    }
}
