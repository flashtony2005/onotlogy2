//! Mature-reasoner facade for the knowledge vocabulary.
//!
//! The local [`crate::tbox::TBox`] remains a fast, conservative index for
//! embedded checks.  This module converts the same append-only ontology into
//! Horned-OWL and delegates soundness-critical DL queries to
//! `owl-dl-reasoner`.  A reasoner is an immutable snapshot, so expensive
//! consistency and classification results are cached on that instance; a new
//! ontology version gets a new reasoner and therefore cannot observe stale
//! conclusions.  Derived conclusions still never mutate the source ontology.

use crate::error::{KnowledgeError, Result};
use crate::explanation::{EXPLANATION_CACHE_CAPACITY, ExplanationLru};
use crate::model::{
    Axiom, ClassExpression, DataFacet, DataRange as ModelDataRange, Ontology, Term,
};
use crate::model::{Datatype, Literal};
use chrono::{DateTime, Utc};
use horned_owl::model::{
    AsymmetricObjectProperty, Build, ClassAssertion, ClassExpression as OwlClassExpression,
    DataProperty, DataPropertyAssertion, DataPropertyDomain, DataPropertyRange, DataRange,
    Datatype as OwlDatatype, DifferentIndividuals, DisjointClasses, EquivalentClasses,
    FacetRestriction as OwlFacetRestriction, FunctionalDataProperty, FunctionalObjectProperty,
    InverseFunctionalObjectProperty, InverseObjectProperties, IrreflexiveObjectProperty,
    Literal as OwlLiteral, MutableOntology, NamedIndividual, NegativeObjectPropertyAssertion,
    ObjectProperty, ObjectPropertyAssertion, ObjectPropertyDomain, ObjectPropertyExpression,
    ObjectPropertyRange, RcStr, ReflexiveObjectProperty, SameIndividual, SubClassOf,
    SubObjectPropertyExpression, SubObjectPropertyOf, SymmetricObjectProperty,
    TransitiveObjectProperty,
};
use horned_owl::ontology::set::SetOntology;
use horned_owl::vocab::Facet as OwlFacet;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, OnceLock};

/// A reasoned view of all named classes in an ontology.
#[derive(Debug, Clone, Default)]
pub struct ReasonedClassification {
    classes: Vec<Term>,
    superclasses: BTreeMap<Term, BTreeSet<Term>>,
    direct_superclasses: BTreeMap<Term, BTreeSet<Term>>,
    unsatisfiable: BTreeSet<Term>,
}

impl ReasonedClassification {
    pub fn classes(&self) -> &[Term] {
        &self.classes
    }

    pub fn superclasses(&self, class: &Term) -> Option<&BTreeSet<Term>> {
        self.superclasses.get(class)
    }

    pub fn direct_superclasses(&self, class: &Term) -> Option<&BTreeSet<Term>> {
        self.direct_superclasses.get(class)
    }

    pub fn unsatisfiable_classes(&self) -> &BTreeSet<Term> {
        &self.unsatisfiable
    }

    pub fn entails_subclass(&self, sub: &Term, sup: &Term) -> bool {
        self.superclasses
            .get(sub)
            .is_some_and(|parents| parents.contains(sup))
    }

    pub fn is_satisfiable(&self, class: &Term) -> bool {
        !self.unsatisfiable.contains(class)
    }
}

/// Realized named types for every declared individual.
#[derive(Debug, Clone, Default)]
pub struct ReasonedRealization {
    individuals: Vec<Term>,
    entailed_types: BTreeMap<Term, BTreeSet<Term>>,
    most_specific_types: BTreeMap<Term, BTreeSet<Term>>,
}

impl ReasonedRealization {
    pub fn individuals(&self) -> &[Term] {
        &self.individuals
    }

    pub fn entailed_types(&self, individual: &Term) -> Option<&BTreeSet<Term>> {
        self.entailed_types.get(individual)
    }

    pub fn most_specific_types(&self, individual: &Term) -> Option<&BTreeSet<Term>> {
        self.most_specific_types.get(individual)
    }
}

const LOCAL_NAME_BASE: &str = "http://example.com/owl#";
type TermAliases = BTreeMap<String, Term>;

fn term_iri(term: &Term) -> String {
    let value = term.as_str();
    if value.contains("://") || value.starts_with("urn:") {
        value.to_owned()
    } else {
        format!("{LOCAL_NAME_BASE}{value}")
    }
}

fn display_term(iri: impl AsRef<str>) -> Term {
    let iri = iri.as_ref();
    Term::new(iri.strip_prefix(LOCAL_NAME_BASE).unwrap_or(iri))
}

/// A facade over the mature OWL DL reasoner.
#[derive(Debug, Clone)]
pub struct Reasoner {
    ontology: Ontology,
    owl: SetOntology<RcStr>,
    aliases: TermAliases,
    consistency: OnceLock<Result<bool>>,
    classification: OnceLock<Result<ReasonedClassification>>,
    pub(crate) explanations: Arc<Mutex<ExplanationLru>>,
}

fn literal_values_equal(left: &Literal, right: &Literal) -> bool {
    match (left, right) {
        (Literal::Boolean(a), Literal::Boolean(b)) => a == b,
        (Literal::Integer(a), Literal::Integer(b)) => a == b,
        (Literal::String(a), Literal::String(b)) => a == b,
        (Literal::Double(a), Literal::Double(b)) => a == b,
        // Runtime canonicalization makes lexical equality meaningful for dates;
        // dateTimes compare as instants so equivalent offsets are entailed.
        (Literal::Decimal(a), Literal::Decimal(b)) => oaip_abi::canonical_decimal(a)
            .zip(oaip_abi::canonical_decimal(b))
            .is_some_and(|(a, b)| a == b),
        (Literal::Date(a), Literal::Date(b)) => a == b,
        (Literal::DateTime(a), Literal::DateTime(b)) => DateTime::parse_from_rfc3339(a)
            .ok()
            .zip(DateTime::parse_from_rfc3339(b).ok())
            .is_some_and(|(a, b)| a.with_timezone(&Utc) == b.with_timezone(&Utc)),
        _ => false,
    }
}

impl Reasoner {
    pub fn new(ontology: &Ontology) -> Result<Self> {
        // The upstream tableau does not propagate data-property domains. Feed
        // its sound derived class assertions to the reasoner while preserving
        // the original ontology unchanged for explanations and audits.
        let materialized = crate::data::materialize_data_domain_assertions(ontology);
        Ok(Self {
            owl: to_horned_owl(&materialized)?,
            aliases: public_terms(ontology),
            ontology: ontology.clone(),
            consistency: OnceLock::new(),
            classification: OnceLock::new(),
            explanations: Arc::new(Mutex::new(ExplanationLru::new(EXPLANATION_CACHE_CAPACITY))),
        })
    }

    /// Restore the caller's term spelling for reasoner-output IRIs.
    fn output_term(&self, iri: &str) -> Term {
        self.aliases
            .get(iri)
            .cloned()
            .unwrap_or_else(|| display_term(iri))
    }

    /// Reference to the immutable source ontology.
    pub fn ontology(&self) -> &Ontology {
        &self.ontology
    }

    #[cfg(test)]
    fn consistency_cache_is_loaded(&self) -> bool {
        self.consistency.get().is_some()
    }

    #[cfg(test)]
    fn classification_cache_is_loaded(&self) -> bool {
        self.classification.get().is_some()
    }

    /// Whether the whole TBox/ABox has at least one model.
    ///
    /// The external tableau does not yet cover every asserted-data conflict, so
    /// combine its tableau answer with the kernel's deterministic checks rather
    /// than reporting an unsound "consistent" result.
    pub fn is_consistent(&self) -> Result<bool> {
        let cached = self.consistency.get_or_init(|| {
            if crate::data::validate_data_consistency(&self.ontology).is_err() {
                return Ok(false);
            }
            reasoner(|| owl_dl_reasoner::is_consistent(&self.owl))
        });
        cached.clone()
    }

    /// Full OWL DL satisfiability of a named class.
    ///
    /// Without a global model no class can have a model, so an inconsistent
    /// knowledge base soundly makes every named class unsatisfiable.
    pub fn is_satisfiable(&self, class: &Term) -> Result<bool> {
        if !self.is_consistent()? {
            return Ok(false);
        }
        reasoner(|| owl_dl_reasoner::is_class_satisfiable(&self.owl, &term_iri(class)))
    }

    /// Full OWL DL subclass entailment (`sub ⊑ sup`).
    pub fn entails_subclass(&self, sub: &Term, sup: &Term) -> Result<bool> {
        if !self.is_consistent()? {
            return Err(KnowledgeError::Invalid(
                "subclass entailment is undefined for an inconsistent ontology".to_owned(),
            ));
        }
        reasoner(|| owl_dl_reasoner::is_subclass_of(&self.owl, &term_iri(sub), &term_iri(sup)))
    }

    /// Open-world instance check against a named class.
    pub fn is_instance_of(&self, class: &Term, individual: &Term) -> Result<bool> {
        reasoner(|| {
            owl_dl_reasoner::is_instance_of(&self.owl, &term_iri(class), &term_iri(individual))
        })
    }

    /// Whether an object-property assertion follows under OWL semantics.
    ///
    /// The mature reasoner has no direct triple-entailment API, so we add the
    /// contrary assertion and ask whether the knowledge base remains possible:
    /// inconsistency proves that the positive assertion is entailed.
    pub fn entails_object_property(
        &self,
        property: &Term,
        subject: &Term,
        object: &Term,
    ) -> Result<bool> {
        if !self.is_consistent()? {
            return Err(KnowledgeError::Invalid(
                "object-property entailment is undefined for an inconsistent ontology".to_owned(),
            ));
        }
        let mut contrary = self.ontology.clone();
        contrary.push(Axiom::NegativeObjectPropertyAssertion {
            property: property.clone(),
            subject: subject.clone(),
            object: object.clone(),
        });
        Ok(!Reasoner::new(&contrary)?.is_consistent()?)
    }

    /// Check a typed data fact, expanding same-individual aliases and
    /// data-property subsumption.
    ///
    /// General datatype entailment remains unavailable in the mature backend,
    /// so this query covers only the asserted/sub-property fragment and does
    /// not claim full datatype reasoning.
    pub fn entails_data_property(
        &self,
        property: &Term,
        individual: &Term,
        value: &Literal,
    ) -> Result<bool> {
        if !self.is_consistent()? {
            return Err(KnowledgeError::Invalid(
                "data-property entailment is undefined for an inconsistent ontology".to_owned(),
            ));
        }

        // Union-find gives the same transitive alias semantics as the data
        // validator; pairwise `SameIndividuals` groups can overlap.
        let mut aliases = BTreeMap::new();
        let mut sub_properties = BTreeMap::new();
        for axiom in self.ontology.axioms() {
            match axiom {
                Axiom::SameIndividuals(terms) if terms.len() >= 2 => {
                    for pair in terms.windows(2) {
                        crate::data::union(&mut aliases, &pair[0], &pair[1]);
                    }
                }
                Axiom::SubDataPropertyOf { sub, sup } => {
                    sub_properties.insert(sub.clone(), sup.clone());
                }
                _ => {}
            }
        }
        let query_alias = crate::data::parents(&mut aliases, individual.clone());
        for axiom in self.ontology.axioms() {
            let Axiom::DataPropertyAssertion {
                property: asserted_property,
                individual: asserted_individual,
                value: asserted_value,
            } = axiom
            else {
                continue;
            };
            if crate::data::property_ancestors(&sub_properties, asserted_property)
                .contains(property)
                && crate::data::parents(&mut aliases, asserted_individual.clone()) == query_alias
                && literal_values_equal(value, asserted_value)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Every declared individual proven to belong to `class`.
    pub fn instances_of(&self, class: &Term) -> Result<BTreeSet<Term>> {
        if !self.is_consistent()? {
            return Err(KnowledgeError::Invalid(
                "instance retrieval is undefined for an inconsistent ontology".to_owned(),
            ));
        }
        let individuals = reasoner(|| owl_dl_reasoner::instances_of(&self.owl, &term_iri(class)))?;
        Ok(individuals
            .into_iter()
            .map(|iri| self.output_term(&iri))
            .collect())
    }

    /// Complete named-class hierarchy and unsatisfiable-class set.
    pub fn classify(&self) -> Result<ReasonedClassification> {
        self.classification
            .get_or_init(|| self.compute_classification())
            .clone()
    }

    /// Compute the full classification without result caching.
    fn compute_classification(&self) -> Result<ReasonedClassification> {
        if !self.is_consistent()? {
            return Err(KnowledgeError::Invalid(
                "classification is undefined for an inconsistent ontology".to_owned(),
            ));
        }
        let classified = reasoner(|| owl_dl_reasoner::classify(&self.owl))?;
        let classes = classified
            .classes()
            .iter()
            .map(|iri| self.output_term(iri.as_str()))
            .collect();
        // The upstream classifier exposes an unsatisfiable set, but its
        // satisfiability probe is the authoritative query API.  Recheck each
        // named class so facade callers never see the classifier miss a
        // tableau-provable bottom concept.
        let mut unsatisfiable = classified
            .unsatisfiable_classes()
            .into_iter()
            .map(|iri| self.output_term(iri))
            .collect::<BTreeSet<_>>();
        for iri in classified.classes() {
            if !reasoner(|| owl_dl_reasoner::is_class_satisfiable(&self.owl, iri.as_str()))? {
                unsatisfiable.insert(self.output_term(iri.as_str()));
            }
        }

        let mut superclasses = BTreeMap::new();
        let mut direct_superclasses = BTreeMap::new();
        for iri in classified.classes() {
            let class = self.output_term(iri.as_str());
            superclasses.insert(
                class.clone(),
                classified
                    .classes()
                    .iter()
                    .filter(|parent| classified.is_subclass(iri.as_str(), parent))
                    .map(|iri| self.output_term(iri.as_str()))
                    .collect(),
            );
            direct_superclasses.insert(
                class,
                classified
                    .direct_subsumers(iri.as_str())
                    .into_iter()
                    .map(|iri| self.output_term(iri))
                    .collect(),
            );
        }

        Ok(ReasonedClassification {
            unsatisfiable,
            superclasses,
            direct_superclasses,
            classes,
        })
    }

    /// Entailed and most-specific named types for every declared individual.
    pub fn realize(&self) -> Result<ReasonedRealization> {
        if !self.is_consistent()? {
            return Err(KnowledgeError::Invalid(
                "realization is undefined for an inconsistent ontology".to_owned(),
            ));
        }
        let realization = reasoner(|| owl_dl_reasoner::realize(&self.owl))?;
        let mut individuals = realization
            .individuals()
            .iter()
            .map(|iri| self.output_term(iri.as_str()))
            .collect::<Vec<_>>();
        individuals.sort();
        let mut entailed_types = BTreeMap::new();
        let mut most_specific_types = BTreeMap::new();
        for (individual, iri) in individuals.iter().zip(realization.individuals()) {
            entailed_types.insert(
                individual.clone(),
                realization
                    .entailed_types(iri)
                    .iter()
                    .map(|iri| self.output_term(iri.as_str()))
                    .collect(),
            );
            most_specific_types.insert(
                individual.clone(),
                realization
                    .most_specific_types(iri)
                    .iter()
                    .map(|iri| self.output_term(iri.as_str()))
                    .collect(),
            );
        }

        Ok(ReasonedRealization {
            entailed_types,
            most_specific_types,
            individuals,
        })
    }
}

/// Preserve the public vocabulary across the reasoner's stable-IRI boundary.
///
/// Compact local names are normalized before tableau queries, but conclusions
/// must return in the same vocabulary the caller authored.  Full IRIs are never
/// rewritten to a local form merely because they share the generated base.
fn public_terms(ontology: &Ontology) -> TermAliases {
    let mut aliases = TermAliases::new();

    fn insert(aliases: &mut TermAliases, term: &Term) {
        aliases
            .entry(term_iri(term))
            .or_insert_with(|| term.clone());
    }

    fn class_terms(expression: &ClassExpression, aliases: &mut TermAliases) {
        match expression {
            ClassExpression::Class(term) => insert(aliases, term),
            ClassExpression::Intersection(items) | ClassExpression::Union(items) => {
                items.iter().for_each(|item| class_terms(item, aliases));
            }
            ClassExpression::Complement(item) => class_terms(item, aliases),
            ClassExpression::ObjectSomeValuesFrom { value, .. }
            | ClassExpression::ObjectAllValuesFrom { value, .. }
            | ClassExpression::ObjectMinCardinality { value, .. }
            | ClassExpression::ObjectMaxCardinality { value, .. } => {
                class_terms(value, aliases);
            }
            ClassExpression::ObjectHasValue { individual, .. } => insert(aliases, individual),
            ClassExpression::Thing
            | ClassExpression::Nothing
            | ClassExpression::DataSomeValuesFrom { .. }
            | ClassExpression::DataAllValuesFrom { .. }
            | ClassExpression::DataHasValue { .. }
            | ClassExpression::DataMinCardinality { .. }
            | ClassExpression::DataMaxCardinality { .. } => {}
        }
    }

    for axiom in ontology.axioms() {
        match axiom {
            Axiom::SubClassOf { sub, sup } => {
                class_terms(sub, &mut aliases);
                class_terms(sup, &mut aliases);
            }
            Axiom::EquivalentClasses(classes) | Axiom::DisjointClasses(classes) => {
                classes
                    .iter()
                    .for_each(|class| class_terms(class, &mut aliases));
            }
            Axiom::ObjectPropertyDomain { class, .. }
            | Axiom::ObjectPropertyRange { class, .. }
            | Axiom::DataPropertyDomain { class, .. } => class_terms(class, &mut aliases),
            Axiom::ClassAssertion { class, individual } => {
                class_terms(class, &mut aliases);
                insert(&mut aliases, individual);
            }
            Axiom::ObjectPropertyAssertion {
                subject, object, ..
            } => {
                insert(&mut aliases, subject);
                insert(&mut aliases, object);
            }
            Axiom::DataPropertyAssertion { individual, .. }
            | Axiom::NegativeDataPropertyAssertion { individual, .. } => {
                insert(&mut aliases, individual);
            }
            Axiom::SameIndividuals(terms) | Axiom::DifferentIndividuals(terms) => {
                terms.iter().for_each(|term| insert(&mut aliases, term));
            }
            _ => {}
        }
    }
    aliases
}

fn reasoner<T>(
    query: impl FnOnce() -> std::result::Result<T, owl_dl_reasoner::ReasonError>,
) -> Result<T> {
    query().map_err(|error| KnowledgeError::Reasoner(error.to_string()))
}

fn to_horned_owl(ontology: &Ontology) -> Result<SetOntology<RcStr>> {
    let build = Build::new_rc();
    let mut out = SetOntology::new_rc();

    for axiom in ontology.axioms() {
        match axiom {
            Axiom::SubClassOf { sub, sup } => {
                let sub = class_expression(sub, &build, &mut out);
                let sup = class_expression(sup, &build, &mut out);
                out.insert(SubClassOf { sub, sup });
            }
            Axiom::EquivalentClasses(classes) if classes.len() >= 2 => {
                let classes = classes
                    .iter()
                    .map(|expr| class_expression(expr, &build, &mut out))
                    .collect();
                out.insert(EquivalentClasses(classes));
            }
            Axiom::DisjointClasses(classes) if classes.len() >= 2 => {
                let classes = classes
                    .iter()
                    .map(|expr| class_expression(expr, &build, &mut out))
                    .collect();
                out.insert(DisjointClasses(classes));
            }
            Axiom::ObjectPropertyDomain { property, class } => {
                declare_property(&build, &mut out, property);
                let ope = property_expression(&build, property);
                let ce = class_expression(class, &build, &mut out);
                out.insert(ObjectPropertyDomain { ope, ce });
            }
            Axiom::ObjectPropertyRange { property, class } => {
                declare_property(&build, &mut out, property);
                let ope = property_expression(&build, property);
                let ce = class_expression(class, &build, &mut out);
                out.insert(ObjectPropertyRange { ope, ce });
            }
            Axiom::SubObjectPropertyOf { sub, sup } => {
                declare_property(&build, &mut out, sub);
                declare_property(&build, &mut out, sup);
                let sup = property_expression(&build, sup);
                let sub = SubObjectPropertyExpression::ObjectPropertyExpression(
                    property_expression(&build, sub),
                );
                out.insert(SubObjectPropertyOf { sup, sub });
            }
            Axiom::TransitiveObjectProperty(property) => {
                declare_property(&build, &mut out, property);
                out.insert(TransitiveObjectProperty(property_expression(
                    &build, property,
                )));
            }
            Axiom::SymmetricObjectProperty(property) => {
                declare_property(&build, &mut out, property);
                out.insert(SymmetricObjectProperty(property_expression(
                    &build, property,
                )));
            }
            Axiom::FunctionalObjectProperty(property) => {
                declare_property(&build, &mut out, property);
                out.insert(FunctionalObjectProperty(property_expression(
                    &build, property,
                )));
            }
            Axiom::InverseFunctionalObjectProperty(property) => {
                declare_property(&build, &mut out, property);
                out.insert(InverseFunctionalObjectProperty(property_expression(
                    &build, property,
                )));
            }
            Axiom::ReflexiveObjectProperty(property) => {
                declare_property(&build, &mut out, property);
                out.insert(ReflexiveObjectProperty(property_expression(
                    &build, property,
                )));
            }
            Axiom::IrreflexiveObjectProperty(property) => {
                declare_property(&build, &mut out, property);
                out.insert(IrreflexiveObjectProperty(property_expression(
                    &build, property,
                )));
            }
            Axiom::AsymmetricObjectProperty(property) => {
                declare_property(&build, &mut out, property);
                out.insert(AsymmetricObjectProperty(property_expression(
                    &build, property,
                )));
            }
            Axiom::InverseObjectProperties { first, second } => {
                declare_property(&build, &mut out, first);
                declare_property(&build, &mut out, second);
                out.insert(InverseObjectProperties(
                    build.object_property(term_iri(first)),
                    build.object_property(term_iri(second)),
                ));
            }
            Axiom::SubDataPropertyOf { sub, sup } => {
                declare_data_property(&build, &mut out, sub);
                declare_data_property(&build, &mut out, sup);
                let sup = build.data_property(term_iri(sup));
                let sub = build.data_property(term_iri(sub));
                out.insert(horned_owl::model::SubDataPropertyOf { sup, sub });
            }
            Axiom::DisjointDataProperties(properties) if properties.len() >= 2 => {
                let properties = properties
                    .iter()
                    .map(|property| {
                        declare_data_property(&build, &mut out, property);
                        build.data_property(term_iri(property))
                    })
                    .collect();
                out.insert(horned_owl::model::DisjointDataProperties(properties));
            }
            Axiom::DataPropertyDomain { property, class } => {
                declare_data_property(&build, &mut out, property);
                let dp = build.data_property(term_iri(property));
                let ce = class_expression(class, &build, &mut out);
                out.insert(DataPropertyDomain { dp, ce });
            }
            Axiom::DataPropertyRange { property, datatype } => {
                declare_data_property(&build, &mut out, property);
                let dp = build.data_property(term_iri(property));
                let dr = data_range(&build, *datatype);
                out.insert(DataPropertyRange { dp, dr });
            }
            Axiom::DataPropertyOneOf { property, values } => {
                declare_data_property(&build, &mut out, property);
                let dp = build.data_property(term_iri(property));
                out.insert(DataPropertyRange {
                    dp,
                    dr: DataRange::DataOneOf(
                        values
                            .iter()
                            .map(|value| owl_literal(&build, value))
                            .collect(),
                    ),
                });
            }
            Axiom::DataPropertyRangeRestriction {
                property,
                datatype,
                facets,
            } => {
                declare_data_property(&build, &mut out, property);
                let dp = build.data_property(term_iri(property));
                out.insert(DataPropertyRange {
                    dp,
                    dr: DataRange::DatatypeRestriction(
                        owl_datatype(&build, *datatype),
                        facets
                            .iter()
                            .map(|value| OwlFacetRestriction {
                                f: owl_facet(value.facet),
                                l: owl_literal(&build, &value.value),
                            })
                            .collect(),
                    ),
                });
            }
            Axiom::DataPropertyDataRange { property, range } => {
                crate::data::validate_data_range(range, property)?;
                declare_data_property(&build, &mut out, property);
                let dr = owl_data_range(&build, range);
                let dp = build.data_property(term_iri(property));
                out.insert(DataPropertyRange { dp, dr });
            }
            Axiom::FunctionalDataProperty(property) => {
                declare_data_property(&build, &mut out, property);
                out.insert(FunctionalDataProperty(
                    build.data_property(term_iri(property)),
                ));
            }
            Axiom::ClassAssertion { class, individual } => {
                let i = named_individual(&build, &mut out, individual).into();
                let ce = class_expression(class, &build, &mut out);
                out.insert(ClassAssertion { ce, i });
            }
            Axiom::ObjectPropertyAssertion {
                property,
                subject,
                object,
            } => {
                declare_property(&build, &mut out, property);
                let from = named_individual(&build, &mut out, subject).into();
                let to = named_individual(&build, &mut out, object).into();
                out.insert(ObjectPropertyAssertion {
                    ope: property_expression(&build, property),
                    from,
                    to,
                });
            }
            Axiom::NegativeObjectPropertyAssertion {
                property,
                subject,
                object,
            } => {
                declare_property(&build, &mut out, property);
                let from = named_individual(&build, &mut out, subject).into();
                let to = named_individual(&build, &mut out, object).into();
                out.insert(NegativeObjectPropertyAssertion {
                    ope: property_expression(&build, property),
                    from,
                    to,
                });
            }
            Axiom::DataPropertyAssertion {
                property,
                individual,
                value,
            } => {
                declare_data_property(&build, &mut out, property);
                let from = named_individual(&build, &mut out, individual).into();
                let to = owl_literal(&build, value);
                out.insert(DataPropertyAssertion {
                    dp: build.data_property(term_iri(property)),
                    from,
                    to,
                });
            }
            Axiom::NegativeDataPropertyAssertion {
                property,
                individual,
                value,
            } => {
                declare_data_property(&build, &mut out, property);
                let from = named_individual(&build, &mut out, individual).into();
                let to = owl_literal(&build, value);
                out.insert(horned_owl::model::NegativeDataPropertyAssertion {
                    dp: build.data_property(term_iri(property)),
                    from,
                    to,
                });
            }
            Axiom::SameIndividuals(terms) if terms.len() >= 2 => {
                let individuals = terms
                    .iter()
                    .map(|term| named_individual(&build, &mut out, term).into())
                    .collect();
                out.insert(SameIndividual(individuals));
            }
            Axiom::DifferentIndividuals(terms) if terms.len() >= 2 => {
                let individuals = terms
                    .iter()
                    .map(|term| named_individual(&build, &mut out, term).into())
                    .collect();
                out.insert(DifferentIndividuals(individuals));
            }
            Axiom::EquivalentClasses(_)
            | Axiom::DisjointClasses(_)
            | Axiom::DisjointDataProperties(_)
            | Axiom::SameIndividuals(_)
            | Axiom::DifferentIndividuals(_) => {
                return Err(KnowledgeError::Invalid(format!(
                    "cardinality axiom needs at least two operands: {axiom:?}"
                )));
            }
        }
    }

    Ok(out)
}

fn owl_data_range(build: &Build<RcStr>, range: &ModelDataRange) -> DataRange<RcStr> {
    match range {
        ModelDataRange::Datatype(datatype) => DataRange::Datatype(owl_datatype(build, *datatype)),
        ModelDataRange::OneOf(values) => DataRange::DataOneOf(
            values
                .iter()
                .map(|value| owl_literal(build, value))
                .collect(),
        ),
        ModelDataRange::Restriction { datatype, facets } => DataRange::DatatypeRestriction(
            owl_datatype(build, *datatype),
            facets
                .iter()
                .map(|value| OwlFacetRestriction {
                    f: owl_facet(value.facet),
                    l: owl_literal(build, &value.value),
                })
                .collect(),
        ),
        ModelDataRange::Union(branches) => DataRange::DataUnionOf(
            branches
                .iter()
                .map(|branch| owl_data_range(build, branch))
                .collect(),
        ),
        ModelDataRange::Intersection(branches) => DataRange::DataIntersectionOf(
            branches
                .iter()
                .map(|branch| owl_data_range(build, branch))
                .collect(),
        ),
        ModelDataRange::Complement(child) => {
            DataRange::DataComplementOf(Box::new(owl_data_range(build, child)))
        }
    }
}

fn class_expression(
    expression: &ClassExpression,
    build: &Build<RcStr>,
    out: &mut SetOntology<RcStr>,
) -> OwlClassExpression<RcStr> {
    match expression {
        ClassExpression::Thing => {
            OwlClassExpression::Class(build.class("http://www.w3.org/2002/07/owl#Thing"))
        }
        ClassExpression::Nothing => {
            OwlClassExpression::Class(build.class("http://www.w3.org/2002/07/owl#Nothing"))
        }
        ClassExpression::Class(term) => {
            out.declare(build.class(term_iri(term)));
            OwlClassExpression::Class(build.class(term_iri(term)))
        }
        ClassExpression::Intersection(items) => OwlClassExpression::ObjectIntersectionOf(
            items
                .iter()
                .map(|item| class_expression(item, build, out))
                .collect(),
        ),
        ClassExpression::Union(items) => OwlClassExpression::ObjectUnionOf(
            items
                .iter()
                .map(|item| class_expression(item, build, out))
                .collect(),
        ),
        ClassExpression::Complement(item) => {
            OwlClassExpression::ObjectComplementOf(Box::new(class_expression(item, build, out)))
        }
        ClassExpression::ObjectSomeValuesFrom { property, value } => {
            declare_property(build, out, property);
            OwlClassExpression::ObjectSomeValuesFrom {
                ope: property_expression(build, property),
                bce: Box::new(class_expression(value, build, out)),
            }
        }
        ClassExpression::ObjectAllValuesFrom { property, value } => {
            declare_property(build, out, property);
            OwlClassExpression::ObjectAllValuesFrom {
                ope: property_expression(build, property),
                bce: Box::new(class_expression(value, build, out)),
            }
        }
        ClassExpression::ObjectHasValue {
            property,
            individual,
        } => {
            declare_property(build, out, property);
            OwlClassExpression::ObjectHasValue {
                ope: property_expression(build, property),
                i: named_individual(build, out, individual).into(),
            }
        }
        ClassExpression::ObjectMinCardinality {
            cardinality,
            property,
            value,
        } => {
            declare_property(build, out, property);
            OwlClassExpression::ObjectMinCardinality {
                n: *cardinality,
                ope: property_expression(build, property),
                bce: Box::new(class_expression(value, build, out)),
            }
        }
        ClassExpression::ObjectMaxCardinality {
            cardinality,
            property,
            value,
        } => {
            declare_property(build, out, property);
            OwlClassExpression::ObjectMaxCardinality {
                n: *cardinality,
                ope: property_expression(build, property),
                bce: Box::new(class_expression(value, build, out)),
            }
        }
        ClassExpression::DataSomeValuesFrom { property, datatype } => {
            declare_data_property(build, out, property);
            OwlClassExpression::DataSomeValuesFrom {
                dp: build.data_property(term_iri(property)),
                dr: data_range(build, *datatype),
            }
        }
        ClassExpression::DataAllValuesFrom { property, datatype } => {
            declare_data_property(build, out, property);
            OwlClassExpression::DataAllValuesFrom {
                dp: build.data_property(term_iri(property)),
                dr: data_range(build, *datatype),
            }
        }
        ClassExpression::DataHasValue { property, value } => {
            declare_data_property(build, out, property);
            OwlClassExpression::DataHasValue {
                dp: build.data_property(term_iri(property)),
                l: owl_literal(build, value),
            }
        }
        ClassExpression::DataMinCardinality {
            cardinality,
            property,
            datatype,
        } => {
            declare_data_property(build, out, property);
            OwlClassExpression::DataMinCardinality {
                n: *cardinality,
                dp: build.data_property(term_iri(property)),
                dr: data_range(build, *datatype),
            }
        }
        ClassExpression::DataMaxCardinality {
            cardinality,
            property,
            datatype,
        } => {
            declare_data_property(build, out, property);
            OwlClassExpression::DataMaxCardinality {
                n: *cardinality,
                dp: build.data_property(term_iri(property)),
                dr: data_range(build, *datatype),
            }
        }
    }
}

fn data_range(build: &Build<RcStr>, datatype: Datatype) -> DataRange<RcStr> {
    DataRange::Datatype(owl_datatype(build, datatype))
}

fn owl_datatype(build: &Build<RcStr>, datatype: Datatype) -> OwlDatatype<RcStr> {
    build.datatype(datatype.iri())
}

fn owl_literal(build: &Build<RcStr>, literal: &Literal) -> OwlLiteral<RcStr> {
    match literal {
        Literal::String(value) => OwlLiteral::Simple {
            literal: value.clone(),
        },
        Literal::Boolean(value) => OwlLiteral::Datatype {
            literal: value.to_string(),
            datatype_iri: build.iri(Datatype::Boolean.iri()),
        },
        Literal::Integer(value) => OwlLiteral::Datatype {
            literal: value.to_string(),
            datatype_iri: build.iri(Datatype::Integer.iri()),
        },
        Literal::Double(value) => OwlLiteral::Datatype {
            literal: format!("{value:?}"),
            datatype_iri: build.iri(Datatype::Double.iri()),
        },
        Literal::Decimal(value) => OwlLiteral::Datatype {
            literal: value.clone(),
            datatype_iri: build.iri(Datatype::Decimal.iri()),
        },
        Literal::Date(value) => OwlLiteral::Datatype {
            literal: value.clone(),
            datatype_iri: build.iri(Datatype::Date.iri()),
        },
        Literal::DateTime(value) => OwlLiteral::Datatype {
            literal: value.clone(),
            datatype_iri: build.iri(Datatype::DateTime.iri()),
        },
    }
}

fn owl_facet(facet: DataFacet) -> OwlFacet {
    match facet {
        DataFacet::MinInclusive => OwlFacet::MinInclusive,
        DataFacet::MinExclusive => OwlFacet::MinExclusive,
        DataFacet::MaxInclusive => OwlFacet::MaxInclusive,
        DataFacet::MaxExclusive => OwlFacet::MaxExclusive,
        DataFacet::MinLength => OwlFacet::MinLength,
        DataFacet::MaxLength => OwlFacet::MaxLength,
        DataFacet::TotalDigits => OwlFacet::TotalDigits,
        DataFacet::FractionDigits => OwlFacet::FractionDigits,
        DataFacet::Pattern => OwlFacet::Pattern,
    }
}

fn declare_data_property(build: &Build<RcStr>, out: &mut SetOntology<RcStr>, term: &Term) {
    let property: DataProperty<RcStr> = build.data_property(term_iri(term));
    out.declare(property);
}

fn property_expression(build: &Build<RcStr>, term: &Term) -> ObjectPropertyExpression<RcStr> {
    ObjectPropertyExpression::ObjectProperty(build.object_property(term_iri(term)))
}

fn declare_property(build: &Build<RcStr>, out: &mut SetOntology<RcStr>, term: &Term) {
    let property: ObjectProperty<RcStr> = build.object_property(term_iri(term));
    out.declare(property);
}

fn named_individual(
    build: &Build<RcStr>,
    out: &mut SetOntology<RcStr>,
    term: &Term,
) -> NamedIndividual<RcStr> {
    let individual: NamedIndividual<RcStr> = build.named_individual(term_iri(term));
    out.declare(individual.clone());
    individual
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Axiom;

    const BASE: &str = "http://example.com/owl#";

    fn class(name: &str) -> ClassExpression {
        ClassExpression::class(format!("{BASE}{name}"))
    }

    fn term(name: &str) -> Term {
        Term::new(format!("{BASE}{name}"))
    }

    fn ontology() -> Ontology {
        Ontology::new()
            .with_axiom(Axiom::subclass(class("Person"), class("Animal")))
            .with_axiom(Axiom::subclass(
                class("Unsat"),
                ClassExpression::intersection([
                    class("Animal"),
                    ClassExpression::complement(class("Animal")),
                ]),
            ))
            .with_axiom(Axiom::ClassAssertion {
                class: class("Person"),
                individual: term("alice"),
            })
            .with_axiom(Axiom::DifferentIndividuals(vec![
                term("alice"),
                term("bob"),
            ]))
    }

    #[test]
    fn mature_reasoner_entails_named_subclasses() {
        let reasoner = Reasoner::new(&ontology()).unwrap();
        assert!(
            reasoner
                .entails_subclass(&term("Person"), &term("Animal"))
                .unwrap()
        );
        assert!(
            !reasoner
                .entails_subclass(&term("Animal"), &term("Person"))
                .unwrap()
        );
    }

    #[test]
    fn immutable_reasoner_caches_expensive_snapshot_results() {
        let reasoner = Reasoner::new(&ontology()).unwrap();
        assert!(!reasoner.consistency_cache_is_loaded());
        assert!(reasoner.is_consistent().unwrap());
        assert!(reasoner.consistency_cache_is_loaded());
        assert!(reasoner.is_consistent().unwrap());

        let first = reasoner.classify().unwrap();
        assert!(reasoner.classification_cache_is_loaded());
        let second = reasoner.classify().unwrap();
        assert_eq!(first.classes(), second.classes());
        assert_eq!(
            first.unsatisfiable_classes(),
            second.unsatisfiable_classes()
        );
    }

    #[test]
    fn composable_data_range_participates_in_mature_consistency() {
        let range = ModelDataRange::union([
            ModelDataRange::Datatype(Datatype::String),
            ModelDataRange::Datatype(Datatype::Integer),
        ]);
        let axiom = |property: &str| Axiom::DataPropertyDataRange {
            property: Term::new(property),
            range: range.clone(),
        };

        let consistent = Ontology::new().with_axiom(axiom("identifier")).with_axiom(
            Axiom::DataPropertyAssertion {
                property: Term::new("identifier"),
                individual: Term::new("a"),
                value: crate::model::Literal::Integer(30),
            },
        );
        assert!(Reasoner::new(&consistent).unwrap().is_consistent().unwrap());

        let inconsistent = consistent.with_axiom(Axiom::DataPropertyAssertion {
            property: Term::new("identifier"),
            individual: Term::new("b"),
            value: crate::model::Literal::Boolean(true),
        });
        assert!(
            !Reasoner::new(&inconsistent)
                .unwrap()
                .is_consistent()
                .unwrap()
        );
    }

    #[test]
    fn complement_data_range_participates_in_mature_consistency() {
        let range = ModelDataRange::Complement(Box::new(ModelDataRange::Restriction {
            datatype: Datatype::String,
            facets: vec![crate::model::FacetValue {
                facet: crate::model::DataFacet::Pattern,
                value: crate::model::Literal::String("[A-Z]{3}-[0-9]{4}".into()),
            }],
        }));
        let axiom = Axiom::DataPropertyDataRange {
            property: Term::new("identifier"),
            range,
        };
        let consistent =
            Ontology::new()
                .with_axiom(axiom)
                .with_axiom(Axiom::DataPropertyAssertion {
                    property: Term::new("identifier"),
                    individual: Term::new("a"),
                    value: crate::model::Literal::String("outside".into()),
                });
        assert!(Reasoner::new(&consistent).unwrap().is_consistent().unwrap());

        let inconsistent = consistent.with_axiom(Axiom::DataPropertyAssertion {
            property: Term::new("identifier"),
            individual: Term::new("b"),
            value: crate::model::Literal::String("ABC-1234".into()),
        });
        assert!(
            !Reasoner::new(&inconsistent)
                .unwrap()
                .is_consistent()
                .unwrap()
        );
    }

    #[test]
    fn mature_reasoner_checks_consistency_and_satisfiability() {
        let reasoner = Reasoner::new(&ontology()).unwrap();
        assert!(reasoner.is_consistent().unwrap());
        assert!(!reasoner.is_satisfiable(&term("Unsat")).unwrap());
        assert!(reasoner.is_satisfiable(&term("Person")).unwrap());
    }

    #[test]
    fn mature_reasoner_classifies_hierarchy() {
        let reasoner = Reasoner::new(&ontology()).unwrap();
        let classified = reasoner.classify().unwrap();
        assert!(
            classified
                .superclasses(&term("Person"))
                .unwrap()
                .contains(&term("Animal"))
        );
        assert_eq!(
            classified.direct_superclasses(&term("Person")).unwrap(),
            &BTreeSet::from([term("Animal")])
        );
        assert_eq!(
            classified.unsatisfiable_classes(),
            &BTreeSet::from([term("Unsat")])
        );
    }

    #[test]
    fn mature_reasoner_realizes_instances_with_open_world_inference() {
        let reasoner = Reasoner::new(&ontology()).unwrap();
        assert!(
            reasoner
                .is_instance_of(&term("Animal"), &term("alice"))
                .unwrap()
        );
        assert!(
            !reasoner
                .is_instance_of(&term("Animal"), &term("bob"))
                .unwrap()
        );

        let instances = reasoner.instances_of(&term("Animal")).unwrap();
        assert_eq!(instances, BTreeSet::from([term("alice")]));

        let realized = reasoner.realize().unwrap();
        assert!(
            realized
                .entailed_types(&term("alice"))
                .unwrap()
                .contains(&term("Animal"))
        );
    }

    #[test]
    fn mature_reasoner_entails_object_property_assertions() {
        let ontology = Ontology::new()
            .with_axiom(Axiom::InverseObjectProperties {
                first: Term::new("parent"),
                second: Term::new("child"),
            })
            .with_axiom(Axiom::ObjectPropertyAssertion {
                property: Term::new("parent"),
                subject: term("alice"),
                object: term("bob"),
            });
        let reasoner = Reasoner::new(&ontology).unwrap();

        assert!(
            reasoner
                .entails_object_property(&Term::new("parent"), &term("alice"), &term("bob"))
                .unwrap()
        );
        assert!(
            reasoner
                .entails_object_property(&Term::new("child"), &term("bob"), &term("alice"))
                .unwrap()
        );
        assert!(
            !reasoner
                .entails_object_property(&Term::new("child"), &term("alice"), &term("bob"))
                .unwrap()
        );
    }

    #[test]
    fn negative_object_property_assertion_is_representable_and_conflicting() {
        let ontology = Ontology::new().with_axiom(Axiom::NegativeObjectPropertyAssertion {
            property: Term::new("parent"),
            subject: term("alice"),
            object: term("bob"),
        });
        let reasoner = Reasoner::new(&ontology).unwrap();
        assert!(reasoner.is_consistent().unwrap());
        assert!(
            !reasoner
                .entails_object_property(&Term::new("parent"), &term("alice"), &term("bob"))
                .unwrap()
        );

        let contradictory = ontology.with_axiom(Axiom::ObjectPropertyAssertion {
            property: Term::new("parent"),
            subject: term("alice"),
            object: term("bob"),
        });
        assert!(
            !Reasoner::new(&contradictory)
                .unwrap()
                .is_consistent()
                .unwrap()
        );
    }

    #[test]
    fn mature_reasoner_rejects_object_property_characteristic_violations() {
        let functional = Ontology::new()
            .with_axiom(Axiom::FunctionalObjectProperty(Term::new("parent")))
            .with_axiom(Axiom::ObjectPropertyAssertion {
                property: Term::new("parent"),
                subject: term("alice"),
                object: term("bob"),
            })
            .with_axiom(Axiom::ObjectPropertyAssertion {
                property: Term::new("parent"),
                subject: term("alice"),
                object: term("carol"),
            })
            .with_axiom(Axiom::DifferentIndividuals(vec![
                term("bob"),
                term("carol"),
            ]));
        assert!(!Reasoner::new(&functional).unwrap().is_consistent().unwrap());

        let asymmetric = Ontology::new()
            .with_axiom(Axiom::AsymmetricObjectProperty(Term::new("parent")))
            .with_axiom(Axiom::ObjectPropertyAssertion {
                property: Term::new("parent"),
                subject: term("alice"),
                object: term("bob"),
            })
            .with_axiom(Axiom::ObjectPropertyAssertion {
                property: Term::new("parent"),
                subject: term("bob"),
                object: term("alice"),
            });
        assert!(!Reasoner::new(&asymmetric).unwrap().is_consistent().unwrap());

        let irreflexive = Ontology::new()
            .with_axiom(Axiom::IrreflexiveObjectProperty(Term::new("parent")))
            .with_axiom(Axiom::ObjectPropertyAssertion {
                property: Term::new("parent"),
                subject: term("alice"),
                object: term("alice"),
            });
        assert!(
            !Reasoner::new(&irreflexive)
                .unwrap()
                .is_consistent()
                .unwrap()
        );
    }

    #[test]
    fn inconsistent_abox_is_rejected_by_mature_reasoner() {
        let ontology = Ontology::new()
            .with_axiom(Axiom::DisjointClasses(vec![class("Male"), class("Female")]))
            .with_axiom(Axiom::ClassAssertion {
                class: class("Male"),
                individual: term("a"),
            })
            .with_axiom(Axiom::ClassAssertion {
                class: class("Female"),
                individual: term("a"),
            });
        assert!(!Reasoner::new(&ontology).unwrap().is_consistent().unwrap());
    }
}

#[cfg(test)]
mod data_tests {
    use super::*;
    use crate::model::{Axiom, Datatype, Literal};

    #[test]
    fn data_property_schema_and_fact_is_representable() {
        let person = Term::new("http://example.com/owl#Person");
        let age = Term::new("http://example.com/owl#age");
        let alice = Term::new("http://example.com/owl#alice");
        let ontology = Ontology::new()
            .with_axiom(Axiom::subclass(
                ClassExpression::Class(person.clone()),
                ClassExpression::Thing,
            ))
            .with_axiom(Axiom::DataPropertyDomain {
                property: age.clone(),
                class: ClassExpression::Class(person.clone()),
            })
            .with_axiom(Axiom::DataPropertyRange {
                property: age.clone(),
                datatype: Datatype::Integer,
            })
            .with_axiom(Axiom::FunctionalDataProperty(age.clone()))
            .with_axiom(Axiom::DataPropertyAssertion {
                property: age.clone(),
                individual: alice.clone(),
                value: Literal::Integer(30),
            });

        let reasoner = Reasoner::new(&ontology).unwrap();
        assert!(reasoner.is_consistent().unwrap());
        // The upstream tableau does not propagate domains, so Reasoner::new
        // supplies this sound consequence before delegation.
        assert!(reasoner.is_instance_of(&person, &alice).unwrap());

        let domain_justification = reasoner
            .explain_instance(&person, &alice)
            .unwrap()
            .expect("domain inference should be explainable");
        assert_eq!(domain_justification.axioms.len(), 2);
        assert!(domain_justification.axioms.iter().any(|axiom| matches!(
            axiom,
            Axiom::DataPropertyDomain { property, .. } if *property == age
        )));
        assert!(domain_justification.axioms.iter().any(|axiom| matches!(
            axiom,
            Axiom::DataPropertyAssertion { property, .. } if *property == age
        )));
    }

    #[test]
    fn data_domain_consequences_can_make_an_abox_inconsistent() {
        let customer = Term::new("http://example.com/owl#Customer");
        let order = Term::new("http://example.com/owl#Order");
        let age = Term::new("http://example.com/owl#age");
        let order_one = Term::new("http://example.com/owl#order-1");
        let ontology = Ontology::new()
            .with_axiom(Axiom::DisjointClasses(vec![
                ClassExpression::Class(customer.clone()),
                ClassExpression::Class(order.clone()),
            ]))
            .with_axiom(Axiom::DataPropertyDomain {
                property: age.clone(),
                class: ClassExpression::Class(customer),
            })
            .with_axiom(Axiom::ClassAssertion {
                class: ClassExpression::Class(order),
                individual: order_one.clone(),
            })
            .with_axiom(Axiom::DataPropertyAssertion {
                property: age,
                individual: order_one,
                value: Literal::Integer(1001),
            });
        assert!(!Reasoner::new(&ontology).unwrap().is_consistent().unwrap());
    }
}

#[cfg(test)]
mod data_negative_tests {
    use super::*;
    use crate::model::{Axiom, Literal};

    #[test]
    fn direct_data_properties_are_entailed_and_explainable() {
        let age = Term::new("http://example.com/owl#age");
        let alice = Term::new("http://example.com/owl#alice");
        let ontology = Ontology::new()
            .with_axiom(Axiom::subclass(
                ClassExpression::class(Term::new("http://example.com/owl#Person")),
                ClassExpression::Thing,
            ))
            .with_axiom(Axiom::DataPropertyAssertion {
                property: age.clone(),
                individual: alice.clone(),
                value: Literal::Integer(30),
            });
        let reasoner = Reasoner::new(&ontology).unwrap();

        assert!(
            reasoner
                .entails_data_property(&age, &alice, &Literal::Integer(30))
                .unwrap()
        );
        assert!(
            !reasoner
                .entails_data_property(&age, &alice, &Literal::Integer(31))
                .unwrap()
        );

        let explanation = reasoner
            .explain_data_property(&age, &alice, &Literal::Integer(30))
            .unwrap()
            .expect("direct data fact should be explainable");
        assert_eq!(vec![1], explanation.axiom_indices);
    }

    #[test]
    fn calendar_dates_and_date_time_instants_are_entailed_by_value() {
        let occurred_at = Term::new("occurred-at");
        let event = Term::new("event-1");
        let ontology = Ontology::new()
            .with_axiom(Axiom::DataPropertyAssertion {
                property: occurred_at.clone(),
                individual: event.clone(),
                value: Literal::DateTime("2026-08-23T08:00:00+08:00".into()),
            })
            .with_axiom(Axiom::DataPropertyAssertion {
                property: Term::new("birthday"),
                individual: event.clone(),
                value: Literal::Date("2024-02-29".into()),
            });
        let reasoner = Reasoner::new(&ontology).unwrap();

        assert!(
            reasoner
                .entails_data_property(
                    &occurred_at,
                    &event,
                    &Literal::DateTime("2026-08-23T00:00:00Z".into())
                )
                .unwrap()
        );
        assert!(
            !reasoner
                .entails_data_property(
                    &occurred_at,
                    &event,
                    &Literal::DateTime("2026-08-23T00:00:01Z".into())
                )
                .unwrap()
        );
        assert!(
            reasoner
                .entails_data_property(
                    &Term::new("birthday"),
                    &event,
                    &Literal::Date("2024-02-29".into())
                )
                .unwrap()
        );
        assert!(
            !reasoner
                .entails_data_property(
                    &Term::new("birthday"),
                    &event,
                    &Literal::Date("2024-02-28".into())
                )
                .unwrap()
        );
    }

    #[test]
    fn same_individual_aliases_are_transitive_for_data_facts() {
        let age = Term::new("age");
        let value = Literal::Integer(30);
        let ontology = Ontology::new()
            .with_axiom(Axiom::SameIndividuals(vec![Term::new("a"), Term::new("b")]))
            .with_axiom(Axiom::SameIndividuals(vec![Term::new("b"), Term::new("c")]))
            .with_axiom(Axiom::DataPropertyAssertion {
                property: age.clone(),
                individual: Term::new("a"),
                value: value.clone(),
            });
        let reasoner = Reasoner::new(&ontology).unwrap();
        assert!(
            reasoner
                .entails_data_property(&age, &Term::new("c"), &value)
                .unwrap()
        );
    }

    #[test]
    fn decimal_entailment_compares_exact_numeric_value() {
        let balance = Term::new("balance");
        let account = Term::new("account-1");
        let ontology = Ontology::new().with_axiom(Axiom::DataPropertyAssertion {
            property: balance.clone(),
            individual: account.clone(),
            value: Literal::Decimal("1.23".into()),
        });
        let reasoner = Reasoner::new(&ontology).unwrap();
        assert!(
            reasoner
                .entails_data_property(&balance, &account, &Literal::Decimal("1.23000".into()))
                .unwrap()
        );
        assert!(
            !reasoner
                .entails_data_property(&balance, &account, &Literal::Decimal("1.23001".into()))
                .unwrap()
        );
    }

    #[test]
    fn data_entailment_refuses_an_inconsistent_knowledge_base() {
        let age = Term::new("age");
        let alice = Term::new("alice");
        let ontology = Ontology::new()
            .with_axiom(Axiom::DataPropertyAssertion {
                property: age.clone(),
                individual: alice.clone(),
                value: Literal::Integer(30),
            })
            .with_axiom(Axiom::NegativeDataPropertyAssertion {
                property: age.clone(),
                individual: alice.clone(),
                value: Literal::Integer(30),
            });
        let reasoner = Reasoner::new(&ontology).unwrap();
        assert!(!reasoner.is_consistent().unwrap());
        let result = reasoner.entails_data_property(&age, &alice, &Literal::Integer(30));
        assert!(matches!(
            result,
            Err(crate::KnowledgeError::Invalid(message))
                if message.contains("inconsistent ontology")
        ));
        assert!(!reasoner.is_satisfiable(&Term::new("Person")).unwrap());
        assert!(matches!(
            reasoner.classify(),
            Err(crate::KnowledgeError::Invalid(message))
                if message.contains("inconsistent ontology")
        ));
    }

    #[test]
    fn data_properties_follow_data_subproperties() {
        let email = Term::new("email");
        let work_email = Term::new("work-email");
        let alice = Term::new("alice");
        let value = Literal::String("a@x".into());
        let ontology = Ontology::new()
            .with_axiom(Axiom::SubDataPropertyOf {
                sub: work_email.clone(),
                sup: email.clone(),
            })
            .with_axiom(Axiom::DataPropertyAssertion {
                property: work_email.clone(),
                individual: alice.clone(),
                value: value.clone(),
            });
        let reasoner = Reasoner::new(&ontology).unwrap();

        assert!(
            reasoner
                .entails_data_property(&work_email, &alice, &value)
                .unwrap()
        );
        assert!(
            reasoner
                .entails_data_property(&email, &alice, &value)
                .unwrap()
        );
        assert!(
            !reasoner
                .entails_data_property(&email, &alice, &Literal::String("b@x".into()))
                .unwrap()
        );

        let justification = reasoner
            .explain_data_property(&email, &alice, &value)
            .unwrap()
            .expect("sub-property entailment should be explainable");
        assert_eq!(justification.axioms.len(), 2);
    }
}
