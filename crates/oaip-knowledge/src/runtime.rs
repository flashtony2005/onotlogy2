//! Canonical projection from executable ODL schemas into the DL vocabulary.
//!
//! Runtime schemas are not a second ontology: every reviewed `OntologyIR` is
//! materialized as named classes, data properties and action-created object
//! properties inside the governed knowledge base.  Keeping this mapping in one
//! place lets evolution replace the previous executable projection without
//! leaving stale schema facts behind.

use crate::error::{KnowledgeError, Result};
use crate::model::{
    Axiom, ClassExpression, DataFacet, DataRange as ModelDataRange, Datatype, FacetValue, Literal,
    Ontology, Term,
};
use crate::reasoner::Reasoner;
use chrono::{NaiveDate, SecondsFormat, Utc};
use oaip_abi::data_range::range_has_extra_base;
use oaip_abi::{
    canonical_decimal,
    ir::{
        DataLiteral as AbiDataLiteral, DataRestrictionKind, ObjectRestrictionKind, OntologyIR,
        PropertyDataRange, PropertyFacet, ScalarType,
    },
    property_cardinality_bounds, property_one_of_json_values, validate_property_constraints,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

fn class(name: impl Into<String>) -> ClassExpression {
    ClassExpression::class(Term::new(name))
}

fn data_literal(literal: &AbiDataLiteral) -> Literal {
    match literal {
        AbiDataLiteral::Boolean(value) => Literal::Boolean(*value),
        AbiDataLiteral::Integer(value) => Literal::Integer(*value),
        AbiDataLiteral::Double(value) => Literal::Double(*value),
        AbiDataLiteral::String(value) => Literal::String(value.clone()),
    }
}

fn scalar_datatype(ty: ScalarType) -> Datatype {
    match ty {
        ScalarType::String => Datatype::String,
        ScalarType::Int => Datatype::Integer,
        ScalarType::Float => Datatype::Double,
        ScalarType::Bool => Datatype::Boolean,
        ScalarType::Date => Datatype::Date,
        ScalarType::DateTime => Datatype::DateTime,
        ScalarType::Decimal => Datatype::Decimal,
    }
}

/// Canonical data-property ranges used to normalize equivalent JSON numerics.
///
/// JSON has one number type while OWL distinguishes `xsd:integer` and
/// `xsd:double`.  ODL accepts `30` for a Float property, so projection must use
/// the declared canonical range instead of letting that boundary choose a
/// different datatype from the executable validator.
fn data_property_ranges(ontology: &Ontology) -> BTreeMap<Term, BTreeSet<Datatype>> {
    ontology
        .axioms()
        .iter()
        .filter_map(|axiom| match axiom {
            Axiom::DataPropertyRange { property, datatype } => {
                Some((property.clone(), BTreeSet::from([*datatype])))
            }
            Axiom::DataPropertyDataRange { property, range } => {
                Some((property.clone(), model_data_range_datatypes(range)))
            }
            _ => None,
        })
        .collect()
}

fn model_data_range_datatypes(range: &ModelDataRange) -> BTreeSet<Datatype> {
    match range {
        ModelDataRange::Datatype(datatype) => BTreeSet::from([*datatype]),
        ModelDataRange::OneOf(values) => values.iter().map(Literal::datatype).collect(),
        ModelDataRange::Restriction { datatype, .. } => BTreeSet::from([*datatype]),
        ModelDataRange::Complement(child) => model_data_range_datatypes(child),
        ModelDataRange::Union(children) | ModelDataRange::Intersection(children) => children
            .iter()
            .flat_map(model_data_range_datatypes)
            .collect(),
    }
}

fn property_facet_value(ty: ScalarType, facet: &PropertyFacet) -> Option<FacetValue> {
    let (data_facet, value) = match facet {
        PropertyFacet::MinCardinality(_)
        | PropertyFacet::MaxCardinality(_)
        | PropertyFacet::OneOf(_)
        | PropertyFacet::Range(_) => return None,
        PropertyFacet::MinInclusive(value) => (DataFacet::MinInclusive, value),
        PropertyFacet::MinExclusive(value) => (DataFacet::MinExclusive, value),
        PropertyFacet::MaxInclusive(value) => (DataFacet::MaxInclusive, value),
        PropertyFacet::MaxExclusive(value) => (DataFacet::MaxExclusive, value),
        PropertyFacet::MinLength(value) => (
            DataFacet::MinLength,
            &value.parse::<i64>().ok()?.to_string(),
        ),
        PropertyFacet::MaxLength(value) => (
            DataFacet::MaxLength,
            &value.parse::<i64>().ok()?.to_string(),
        ),
        PropertyFacet::TotalDigits(value) => (
            DataFacet::TotalDigits,
            &value.parse::<i64>().ok()?.to_string(),
        ),
        PropertyFacet::FractionDigits(value) => (
            DataFacet::FractionDigits,
            &value.parse::<i64>().ok()?.to_string(),
        ),
        PropertyFacet::Pattern(value) => (DataFacet::Pattern, value),
    };

    let literal = match data_facet {
        DataFacet::MinInclusive
        | DataFacet::MinExclusive
        | DataFacet::MaxInclusive
        | DataFacet::MaxExclusive => match ty {
            ScalarType::Int => Literal::Integer(value.parse().ok()?),
            ScalarType::Float => Literal::Double(value.parse().ok()?),
            ScalarType::Decimal => Literal::Decimal(canonical_decimal(value)?),
            ScalarType::Date => Literal::Date(value.clone()),
            ScalarType::DateTime => Literal::DateTime(value.clone()),
            _ => return None,
        },
        DataFacet::MinLength
        | DataFacet::MaxLength
        | DataFacet::TotalDigits
        | DataFacet::FractionDigits => Literal::Integer(value.parse().ok()?),
        DataFacet::Pattern => Literal::String(value.clone()),
    };
    Some(FacetValue {
        facet: data_facet,
        value: literal,
    })
}

/// Project an ODL composable range into the kernel's OWL data-range syntax.
fn property_data_range(
    ty: ScalarType,
    range: &PropertyDataRange,
) -> std::result::Result<ModelDataRange, KnowledgeError> {
    let datatype = scalar_datatype(ty);
    let range = match range {
        PropertyDataRange::Datatype => ModelDataRange::Datatype(datatype),
        PropertyDataRange::Base(base) => ModelDataRange::Datatype(scalar_datatype(*base)),
        PropertyDataRange::OneOf(values) => {
            let literals = property_one_of_json_values(ty, &[PropertyFacet::OneOf(values.clone())])
                .map_err(KnowledgeError::Invalid)?
                .iter()
                .map(|value| json_literal("range", value, Some(datatype)))
                .collect::<Result<Vec<_>>>()?;
            ModelDataRange::OneOf(literals)
        }
        PropertyDataRange::Restriction(facets) => {
            let mut converted = Vec::with_capacity(facets.len());
            for facet in facets {
                converted.push(property_facet_value(ty, facet).ok_or_else(|| {
                    KnowledgeError::Invalid(format!(
                        "property range facet `{}` is not compatible with `{ty}`",
                        facet.name()
                    ))
                })?);
            }
            ModelDataRange::Restriction {
                datatype,
                facets: converted,
            }
        }
        PropertyDataRange::Union(children) => ModelDataRange::Union(
            children
                .iter()
                .map(|child| property_data_range(ty, child))
                .collect::<Result<Vec<_>>>()?,
        ),
        PropertyDataRange::Intersection(children) => ModelDataRange::Intersection(
            children
                .iter()
                .map(|child| property_data_range(ty, child))
                .collect::<Result<Vec<_>>>()?,
        ),
        PropertyDataRange::Complement(child) => {
            ModelDataRange::Complement(Box::new(property_data_range(ty, child)?))
        }
    };
    Ok(range)
}

/// Deterministic DL schema generated by an executable ODL schema.
pub fn runtime_ir_to_knowledge(ir: &OntologyIR) -> Ontology {
    let mut ontology = Ontology::new();
    let mut classes = ir
        .object_types
        .iter()
        .map(|object_type| object_type.name.clone())
        .collect::<BTreeSet<_>>();
    for object_type in &ir.object_types {
        for action in &object_type.actions {
            for created in &action.creates {
                classes.insert(created.object_type.clone());
            }
        }
    }
    for name in classes {
        ontology.push(Axiom::subclass(class(name), ClassExpression::Thing));
    }
    for object_type in &ir.object_types {
        let source = class(&object_type.name);
        for parent in &object_type.extends {
            ontology.push(Axiom::subclass(source.clone(), class(parent)));
        }
        for property in &object_type.properties {
            let term = Term::new(&property.name);
            ontology.push(Axiom::DataPropertyDomain {
                property: term.clone(),
                class: source.clone(),
            });
            let has_cross_range = property.constraints.iter().any(
                |facet| matches!(facet, PropertyFacet::Range(range) if range_has_extra_base(range)),
            );
            if !has_cross_range {
                ontology.push(Axiom::DataPropertyRange {
                    property: term.clone(),
                    datatype: scalar_datatype(property.ty),
                });
            }
            let value_constraints = property
                .constraints
                .iter()
                .filter(|facet| {
                    !matches!(
                        facet,
                        PropertyFacet::MinCardinality(_)
                            | PropertyFacet::MaxCardinality(_)
                            | PropertyFacet::OneOf(_)
                            | PropertyFacet::Range(_)
                    )
                })
                .collect::<Vec<_>>();
            if !value_constraints.is_empty() {
                let mut facets = Vec::with_capacity(property.constraints.len());
                for constraint in value_constraints {
                    facets.push(
                        property_facet_value(property.ty, constraint).expect(
                            "validated runtime IR contains only declared-compatible facets",
                        ),
                    );
                }
                ontology.push(Axiom::DataPropertyRangeRestriction {
                    property: term.clone(),
                    datatype: scalar_datatype(property.ty),
                    facets,
                });
            }
            for range in property.constraints.iter().filter_map(|facet| match facet {
                PropertyFacet::Range(range) => Some(range),
                _ => None,
            }) {
                ontology.push(Axiom::DataPropertyDataRange {
                    property: term.clone(),
                    range: property_data_range(property.ty, range)
                        .expect("validated runtime IR contains a projectable composable range"),
                });
            }
            if property
                .constraints
                .iter()
                .any(|facet| matches!(facet, PropertyFacet::OneOf(_)))
            {
                let literals = property_one_of_json_values(property.ty, &property.constraints)
                    .expect("validated runtime IR contains valid enumeration values")
                    .iter()
                    .map(|value| json_literal("one_of", value, Some(scalar_datatype(property.ty))))
                    .collect::<std::result::Result<Vec<_>, KnowledgeError>>()
                    .expect("validated runtime IR enumeration values project as OWL literals");
                ontology.push(Axiom::DataPropertyOneOf {
                    property: term.clone(),
                    values: literals,
                });
            }
            ontology.push(Axiom::FunctionalDataProperty(term.clone()));
            let (min_cardinality, max_cardinality) =
                property_cardinality_bounds(&property.constraints)
                    .expect("validated runtime IR contains valid scalar cardinalities");
            if min_cardinality > 0 {
                ontology.push(Axiom::subclass(
                    source.clone(),
                    ClassExpression::DataMinCardinality {
                        cardinality: min_cardinality,
                        property: term.clone(),
                        datatype: scalar_datatype(property.ty),
                    },
                ));
            }
            if max_cardinality < 1 {
                ontology.push(Axiom::subclass(
                    source.clone(),
                    ClassExpression::DataMaxCardinality {
                        cardinality: max_cardinality,
                        property: term.clone(),
                        datatype: scalar_datatype(property.ty),
                    },
                ));
            }
        }
    }
    for group in &ir.disjoint_groups {
        ontology.push(Axiom::DisjointClasses(group.iter().map(class).collect()));
    }
    for group in &ir.equivalent_groups {
        ontology.push(Axiom::EquivalentClasses(group.iter().map(class).collect()));
    }
    for group in &ir.disjoint_unions {
        ontology.push(Axiom::EquivalentClasses(vec![
            class(&group.class),
            ClassExpression::Union(group.members.iter().map(class).collect()),
        ]));
        ontology.push(Axiom::DisjointClasses(
            group.members.iter().map(class).collect(),
        ));
    }
    for group in &ir.object_one_ofs {
        ontology.push(Axiom::EquivalentClasses(vec![
            class(&group.class),
            ClassExpression::object_one_of(
                group.individuals.iter().map(Term::new).collect::<Vec<_>>(),
            ),
        ]));
    }
    for object_type in &ir.object_types {
        let source = class(&object_type.name);
        for restriction in &object_type.restrictions {
            let expression = match &restriction.kind {
                ObjectRestrictionKind::Some { class: filler } => {
                    ClassExpression::some_values_from(restriction.relation.as_str(), class(filler))
                }
                ObjectRestrictionKind::Only { class: filler } => {
                    ClassExpression::all_values_from(restriction.relation.as_str(), class(filler))
                }
                ObjectRestrictionKind::Value { individual } => {
                    ClassExpression::has_value(restriction.relation.as_str(), individual.as_str())
                }
                ObjectRestrictionKind::Min {
                    cardinality,
                    class: filler,
                } => ClassExpression::min_cardinality(
                    *cardinality,
                    restriction.relation.as_str(),
                    class(filler),
                ),
                ObjectRestrictionKind::Max {
                    cardinality,
                    class: filler,
                } => ClassExpression::max_cardinality(
                    *cardinality,
                    restriction.relation.as_str(),
                    class(filler),
                ),
            };
            ontology.push(Axiom::subclass(source.clone(), expression));
        }
        for restriction in &object_type.data_restrictions {
            let expression = match &restriction.kind {
                DataRestrictionKind::Some { datatype } => ClassExpression::some_data(
                    restriction.property.as_str(),
                    scalar_datatype(*datatype),
                ),
                DataRestrictionKind::Only { datatype } => ClassExpression::all_data(
                    restriction.property.as_str(),
                    scalar_datatype(*datatype),
                ),
                DataRestrictionKind::Value { literal } => {
                    ClassExpression::has_data(restriction.property.as_str(), data_literal(literal))
                }
            };
            ontology.push(Axiom::subclass(source.clone(), expression));
        }
    }
    for hierarchy in &ir.relation_hierarchy {
        ontology.push(Axiom::SubObjectPropertyOf {
            sub: Term::new(&hierarchy.sub_relation),
            sup: Term::new(&hierarchy.super_relation),
        });
    }
    for group in &ir.disjoint_relation_groups {
        ontology.push(Axiom::DisjointObjectProperties(
            group.iter().map(Term::new).collect(),
        ));
    }
    // Equivalence is projected through existing ordered inclusions so the
    // canonical axiom set and mature reasoner path remain unchanged.
    for group in &ir.equivalent_relation_groups {
        for sub in group {
            for sup in group {
                if sub == sup {
                    continue;
                }
                ontology.push(Axiom::SubObjectPropertyOf {
                    sub: Term::new(sub),
                    sup: Term::new(sup),
                });
            }
        }
    }
    for hierarchy in &ir.data_property_hierarchy {
        ontology.push(Axiom::SubDataPropertyOf {
            sub: Term::new(&hierarchy.sub_property),
            sup: Term::new(&hierarchy.super_property),
        });
    }
    for group in &ir.disjoint_data_property_groups {
        ontology.push(Axiom::DisjointDataProperties(
            group.iter().map(Term::new).collect(),
        ));
    }
    // Equivalence reuses the same ordered-inclusion projection as object
    // properties, so mature reasoning and deterministic data checks see one
    // canonical semantics.
    for group in &ir.equivalent_data_property_groups {
        for sub in group {
            for sup in group {
                if sub == sup {
                    continue;
                }
                ontology.push(Axiom::SubDataPropertyOf {
                    sub: Term::new(sub),
                    sup: Term::new(sup),
                });
            }
        }
    }
    let cardinalities = ir
        .relations
        .iter()
        .map(|relation| (relation.name.as_str(), relation))
        .collect::<BTreeMap<_, _>>();
    for (edge_name, (source_name, target_name)) in relation_endpoints(ir) {
        let edge = Term::new(&edge_name);
        let source = class(source_name);
        ontology.push(Axiom::ObjectPropertyDomain {
            property: edge.clone(),
            class: source.clone(),
        });
        ontology.push(Axiom::ObjectPropertyRange {
            property: edge.clone(),
            class: class(&target_name),
        });
        if let Some(relation) = cardinalities.get(edge_name.as_str())
            && !relation.composed_of.is_empty()
        {
            ontology.push(Axiom::SubObjectPropertyChainOf {
                properties: relation.composed_of.iter().map(Term::new).collect(),
                super_property: edge.clone(),
            });
        }
        if let Some(relation) = cardinalities.get(edge_name.as_str()) {
            if let Some(cardinality) = relation.min_cardinality {
                ontology.push(Axiom::subclass(
                    source.clone(),
                    ClassExpression::ObjectMinCardinality {
                        cardinality,
                        property: edge.clone(),
                        value: Box::new(class(&target_name)),
                    },
                ));
            }
            if let Some(cardinality) = relation.max_cardinality {
                ontology.push(Axiom::subclass(
                    source,
                    ClassExpression::ObjectMaxCardinality {
                        cardinality,
                        property: edge,
                        value: Box::new(class(&target_name)),
                    },
                ));
            }
        }
    }
    let mut inverse_pairs = BTreeSet::new();
    for relation in &ir.relations {
        let term = Term::new(&relation.name);
        if relation.functional {
            ontology.push(Axiom::FunctionalObjectProperty(term.clone()));
        }
        if relation.inverse_functional {
            ontology.push(Axiom::InverseFunctionalObjectProperty(term.clone()));
        }
        if relation.symmetric {
            ontology.push(Axiom::SymmetricObjectProperty(term.clone()));
        }
        if relation.transitive {
            ontology.push(Axiom::TransitiveObjectProperty(term.clone()));
        }
        if relation.reflexive {
            ontology.push(Axiom::ReflexiveObjectProperty(term.clone()));
        }
        if relation.irreflexive {
            ontology.push(Axiom::IrreflexiveObjectProperty(term.clone()));
        }
        if relation.asymmetric {
            ontology.push(Axiom::AsymmetricObjectProperty(term.clone()));
        }
        if let Some(inverse_name) = relation.inverse_of.as_ref() {
            let pair = if relation.name <= *inverse_name {
                (relation.name.clone(), inverse_name.clone())
            } else {
                (inverse_name.clone(), relation.name.clone())
            };
            if inverse_pairs.insert(pair) {
                ontology.push(Axiom::InverseObjectProperties {
                    first: term,
                    second: Term::new(inverse_name),
                });
            }
        }
    }
    ontology
}

/// Typed source/target endpoints for every governed relation name.
///
/// Explicit relations and action-created edges share one namespace.  The map is
/// deterministic so replacement removes exactly the same axioms that apply added.
pub fn relation_endpoints(ir: &OntologyIR) -> BTreeMap<String, (String, String)> {
    let mut endpoints = BTreeMap::new();
    for relation in &ir.relations {
        endpoints.insert(
            relation.name.clone(),
            (relation.source.clone(), relation.target.clone()),
        );
    }
    for object_type in &ir.object_types {
        for action in &object_type.actions {
            for created in &action.creates {
                let Some(edge) = created.link_edge.as_ref() else {
                    continue;
                };
                let value = (object_type.name.clone(), created.object_type.clone());
                match endpoints.get(edge) {
                    Some(previous) if *previous == value => {}
                    None => {
                        endpoints.insert(edge.clone(), value);
                    }
                    // Invalid conflicts are rejected by `validate_runtime_ir`;
                    // projection remains total so tests can inspect generated syntax.
                    Some(_) => {}
                }
            }
        }
    }
    endpoints
}

/// Reject ambiguous or undeclared relation schemas before they enter governance.
pub fn validate_runtime_ir(ir: &OntologyIR) -> Result<()> {
    let mut object_names = BTreeSet::new();
    for object in &ir.object_types {
        if object.name.trim().is_empty() {
            return Err(KnowledgeError::Invalid(
                "object type name is empty".to_owned(),
            ));
        }
        if !object_names.insert(object.name.as_str()) {
            return Err(KnowledgeError::Invalid(format!(
                "duplicate object type `{}`",
                object.name
            )));
        }
        for property in &object.properties {
            validate_property_constraints(&property.name, property.ty, &property.constraints)
                .map_err(|error| {
                    KnowledgeError::Invalid(format!(
                        "object `{}` property `{}`: {error}",
                        object.name, property.name
                    ))
                })?;
        }
    }
    if let Err(error) = ir.validate_extends() {
        return Err(KnowledgeError::Invalid(format!(
            "object `{}`: {}",
            error.object, error.message
        )));
    }
    if let Err(error) = ir.validate_class_groups() {
        return Err(KnowledgeError::Invalid(error.message));
    }
    if let Err(error) = ir.validate_disjoint_unions() {
        return Err(KnowledgeError::Invalid(error.message));
    }
    if let Err(error) = ir.validate_object_one_ofs() {
        return Err(KnowledgeError::Invalid(error.message));
    }
    if let Err(error) = ir.validate_object_restrictions() {
        return Err(KnowledgeError::Invalid(format!(
            "object `{}`: {}",
            error.object, error.message
        )));
    }
    if let Err(error) = ir.validate_data_restrictions() {
        return Err(KnowledgeError::Invalid(format!(
            "object `{}`: {}",
            error.object, error.message
        )));
    }
    if let Err(error) = ir.validate_relation_cardinalities() {
        return Err(KnowledgeError::Invalid(error));
    }

    let mut relation_names = BTreeSet::new();
    for relation in &ir.relations {
        if relation.name.trim().is_empty() {
            return Err(KnowledgeError::Invalid("relation name is empty".to_owned()));
        }
        if !relation_names.insert(relation.name.as_str()) {
            return Err(KnowledgeError::Invalid(format!(
                "duplicate relation `{}`",
                relation.name
            )));
        }
        if relation.symmetric && relation.asymmetric || relation.reflexive && relation.irreflexive {
            return Err(KnowledgeError::Invalid(format!(
                "relation `{}` has contradictory characteristics",
                relation.name
            )));
        }
        if !object_names.contains(relation.source.as_str())
            || !object_names.contains(relation.target.as_str())
        {
            return Err(KnowledgeError::Invalid(format!(
                "relation `{}` references an undeclared object type",
                relation.name
            )));
        }
    }

    let relation_names = relation_names.iter().copied().collect();
    if let Err(error) = ir.validate_relation_property_groups(&relation_names) {
        return Err(KnowledgeError::Invalid(error.message));
    }
    if let Err(error) = ir.validate_equivalent_relation_groups(&relation_names) {
        return Err(KnowledgeError::Invalid(error.message));
    }

    let data_property_names = ir
        .object_types
        .iter()
        .flat_map(|object| {
            object
                .properties
                .iter()
                .map(|property| property.name.as_str())
        })
        .collect::<BTreeSet<_>>();
    if let Err(error) = ir.validate_data_property_groups(&data_property_names) {
        return Err(KnowledgeError::Invalid(error.message));
    }
    if let Err(error) = ir.validate_equivalent_data_property_groups(&data_property_names) {
        return Err(KnowledgeError::Invalid(error.message));
    }

    for relation in &ir.relations {
        let Some(inverse_name) = relation.inverse_of.as_ref() else {
            continue;
        };
        if inverse_name == &relation.name || !relation_names.contains(inverse_name.as_str()) {
            return Err(KnowledgeError::Invalid(format!(
                "relation `{}` references an invalid inverse relation",
                relation.name
            )));
        }
    }
    if let Err(error) = ir.validate_inverse_relations() {
        return Err(KnowledgeError::Invalid(format!(
            "relation `{}`: {}",
            error.relation, error.message
        )));
    }

    let mut endpoints = ir
        .relations
        .iter()
        .map(|relation| {
            (
                relation.name.as_str(),
                (relation.source.as_str(), relation.target.as_str()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    for object in &ir.object_types {
        for action in &object.actions {
            for created in &action.creates {
                let Some(edge) = created.link_edge.as_ref() else {
                    continue;
                };
                let value = (object.name.as_str(), created.object_type.as_str());
                match endpoints.get(edge.as_str()) {
                    Some(previous) if *previous == value => {}
                    None => {
                        endpoints.insert(edge.as_str(), value);
                    }
                    Some(_) => {
                        return Err(KnowledgeError::Invalid(format!(
                            "action edge `{edge}` conflicts with a declared relation"
                        )));
                    }
                }
            }
        }
    }
    for relation in &ir.relations {
        if relation.composed_of.is_empty() {
            continue;
        }
        if relation.composed_of.len() < 2 {
            return Err(KnowledgeError::Invalid(format!(
                "relation `{}` must compose at least two object properties",
                relation.name
            )));
        }
        for property in &relation.composed_of {
            if !endpoints.contains_key(property.as_str()) {
                return Err(KnowledgeError::Invalid(format!(
                    "relation `{}` composes undeclared object property `{property}`",
                    relation.name
                )));
            }
        }
    }
    Ok(())
}

fn push_unique(ontology: &mut Ontology, axioms: impl IntoIterator<Item = Axiom>) {
    for axiom in axioms {
        if !ontology.axioms().contains(&axiom) {
            ontology.push(axiom);
        }
    }
}

/// Remove the exact deterministic projection of a prior executable schema.
///
/// Axioms identical to generated ones may have been authored directly in the DL
/// layer.  They are treated as part of the runtime projection during replacement
/// because the two representations cannot safely diverge.
pub fn strip_runtime_ir_from_knowledge(
    ontology: &Ontology,
    runtime_ir: Option<&OntologyIR>,
) -> Ontology {
    let Some(runtime_ir) = runtime_ir else {
        return ontology.clone();
    };
    let generated = runtime_ir_to_knowledge(runtime_ir);
    Ontology::from_iter(
        ontology
            .axioms()
            .iter()
            .filter(|axiom| !generated.axioms().contains(axiom))
            .cloned(),
    )
}

/// Combine governed DL axioms with the active executable-schema projection.
pub fn canonical_runtime_ontology(
    ontology: &Ontology,
    previous_runtime_ir: Option<&OntologyIR>,
    active_runtime_ir: Option<&OntologyIR>,
) -> Ontology {
    let mut canonical = strip_runtime_ir_from_knowledge(ontology, previous_runtime_ir);
    if let Some(runtime_ir) = active_runtime_ir {
        push_unique(
            &mut canonical,
            runtime_ir_to_knowledge(runtime_ir).axioms().to_vec(),
        );
    }
    canonical
}

/// A closed-world participation violation found in an authoritative world snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationCardinalityViolation {
    pub individual: String,
    pub property: String,
    pub target_class: String,
    pub min: u32,
    pub max: u32,
    pub actual: u32,
}

/// A closed-world scalar-value presence violation found in a world snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataCardinalityViolation {
    pub individual: String,
    pub property: String,
    pub datatype: Datatype,
    pub min: u32,
    pub max: u32,
    pub actual: u32,
}

/// A storage-independent world object fact awaiting DL materialization.
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeObjectFact {
    pub id: String,
    pub object_type: String,
    pub data: Value,
}

/// A storage-independent world relation fact awaiting DL materialization.
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeRelationFact {
    pub source: String,
    pub relation: String,
    pub target: String,
}

fn date_time_literal(field: &str, value: &str) -> Result<Literal> {
    let parsed = chrono::DateTime::parse_from_rfc3339(value).map_err(|_| {
        KnowledgeError::Invalid(format!("data field `{field}` is not an RFC 3339 dateTime"))
    })?;
    Ok(Literal::DateTime(
        parsed
            .with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::AutoSi, true),
    ))
}

fn decimal_literal(field: &str, value: &str) -> Result<Literal> {
    canonical_decimal(value)
        .map(Literal::Decimal)
        .ok_or_else(|| {
            KnowledgeError::Invalid(format!("data field `{field}` is not an exact xsd:decimal"))
        })
}

fn date_literal(field: &str, value: &str) -> Result<Literal> {
    let parsed = NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| {
        KnowledgeError::Invalid(format!("data field `{field}` is not a calendar date"))
    })?;
    let canonical = parsed.to_string();
    if value != canonical {
        return Err(KnowledgeError::Invalid(format!(
            "data field `{field}` is not a `YYYY-MM-DD` calendar date"
        )));
    }
    Ok(Literal::Date(canonical))
}

fn json_literal(field: &str, value: &Value, expected: Option<Datatype>) -> Result<Literal> {
    match value {
        Value::String(value) if expected == Some(Datatype::Decimal) => {
            decimal_literal(field, value)
        }
        Value::String(value) if expected == Some(Datatype::Date) => date_literal(field, value),
        Value::String(value) if expected == Some(Datatype::DateTime) => {
            date_time_literal(field, value)
        }
        Value::String(value) => Ok(Literal::String(value.clone())),
        Value::Bool(value) => Ok(Literal::Boolean(*value)),
        Value::Number(value) => {
            if expected == Some(Datatype::Double)
                && let Some(value) = value.as_f64()
            {
                return Ok(Literal::Double(value));
            }
            if expected == Some(Datatype::Decimal) {
                if !value.is_i64() && !value.is_u64() {
                    return Err(KnowledgeError::Invalid(format!(
                        "data field `{field}` requires an integral number or exact decimal string"
                    )));
                }
                return decimal_literal(field, &value.to_string());
            }
            if let Some(value) = value.as_i64() {
                Ok(Literal::Integer(value))
            } else {
                let value = value.as_f64().ok_or_else(|| {
                    KnowledgeError::Invalid(format!("data field `{field}` is not finite"))
                })?;
                Ok(Literal::Double(value))
            }
        }
        Value::Null => Err(KnowledgeError::Invalid(format!(
            "data field `{field}` is null; absence must be represented by omitting the field"
        ))),
        _ => Err(KnowledgeError::Invalid(format!(
            "data field `{field}` is not a scalar OWL literal"
        ))),
    }
}

/// Convert JSON while respecting every datatype admitted by a composable range.
///
/// Ordinary single-range properties retain the old canonicalization behavior:
/// for example integral JSON is still projected as `xsd:double` on a Float
/// property.  Cross-datatype unions instead choose the branch matching the
/// concrete JSON value, with explicit date/date-time/decimal conversion only
/// when those branches are declared.
fn json_literal_with_expected_types(
    field: &str,
    value: &Value,
    expected: &BTreeSet<Datatype>,
) -> Result<Literal> {
    let inferred = json_literal(field, value, None)?;
    if expected.is_empty() || expected.contains(&inferred.datatype()) {
        return Ok(inferred);
    }
    for datatype in expected {
        if matches!(
            datatype,
            Datatype::Decimal | Datatype::Date | Datatype::DateTime | Datatype::Double
        ) && let Ok(literal) = json_literal(field, value, Some(*datatype))
        {
            return Ok(literal);
        }
    }
    Err(KnowledgeError::Invalid(format!(
        "data field `{field}` does not match any declared range datatype"
    )))
}

fn runtime_object_axioms(
    fact: &RuntimeObjectFact,
    ranges: &BTreeMap<Term, BTreeSet<Datatype>>,
) -> Result<Vec<Axiom>> {
    let mut axioms = vec![Axiom::ClassAssertion {
        class: class(&fact.object_type),
        individual: Term::new(&fact.id),
    }];
    let Some(fields) = fact.data.as_object() else {
        return Err(KnowledgeError::Invalid(format!(
            "object {} data must be a JSON object",
            fact.id
        )));
    };
    let mut fields = fields.iter().collect::<Vec<_>>();
    fields.sort_by(|left, right| left.0.cmp(right.0));
    for (field, value) in fields {
        if value.is_null() {
            continue;
        }
        let expected = ranges
            .get(&Term::new(field.clone()))
            .cloned()
            .unwrap_or_default();
        axioms.push(Axiom::DataPropertyAssertion {
            property: Term::new(field.clone()),
            individual: Term::new(&fact.id),
            value: json_literal_with_expected_types(field, value, &expected)?,
        });
    }
    Ok(axioms)
}

/// Append the authoritative live-world snapshot to a canonical schema copy.
pub fn runtime_world_to_knowledge(
    ontology: &Ontology,
    objects: &[RuntimeObjectFact],
    relations: &[RuntimeRelationFact],
) -> Result<Ontology> {
    let ranges = data_property_ranges(ontology);
    let mut knowledge_base = ontology.clone();
    for object in objects {
        for axiom in runtime_object_axioms(object, &ranges)? {
            knowledge_base.push(axiom);
        }
    }
    // Storage identifiers name distinct real-world entities.  Without this
    // assertion, OWL may satisfy a functional relation by identifying two IDs.
    if let Some(first) = objects.first()
        && objects.len() > 1
    {
        knowledge_base.push(Axiom::DifferentIndividuals(
            std::iter::once(Term::new(&first.id))
                .chain(objects.iter().skip(1).map(|object| Term::new(&object.id)))
                .collect(),
        ));
    }
    for relation in relations {
        knowledge_base.push(Axiom::ObjectPropertyAssertion {
            property: Term::new(&relation.relation),
            subject: Term::new(&relation.source),
            object: Term::new(&relation.target),
        });
    }
    Ok(knowledge_base)
}

/// Validate the canonical TBox with the mature reasoner before persistence.
pub fn validate_canonical_ontology(ontology: &Ontology) -> Result<()> {
    crate::data::validate_data_consistency(ontology)?;
    if Reasoner::new(ontology)?.is_consistent()? {
        Ok(())
    } else {
        Err(KnowledgeError::Invalid(
            "canonical ontology is inconsistent".to_owned(),
        ))
    }
}

/// Validate a canonical TBox together with the immutable world snapshot.
///
/// Evolution gates must see the same closed set of facts that reasoning APIs
/// will later read; otherwise a proposal can pass schema review and immediately
/// make the live knowledge base inconsistent.
fn named_class_edges(ontology: &Ontology) -> BTreeMap<String, BTreeSet<String>> {
    let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut link = |from: &str, to: &str| {
        edges
            .entry(from.to_owned())
            .or_default()
            .insert(to.to_owned());
    };
    for axiom in ontology.axioms() {
        match axiom {
            Axiom::SubClassOf { sub, sup } => {
                if let (ClassExpression::Class(from), ClassExpression::Class(to)) = (sub, sup) {
                    link(&from.0, &to.0);
                }
            }
            Axiom::EquivalentClasses(classes) => {
                for left in classes {
                    let ClassExpression::Class(left) = left else {
                        continue;
                    };
                    for right in classes {
                        let ClassExpression::Class(right) = right else {
                            continue;
                        };
                        if left != right {
                            link(&left.0, &right.0);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    edges
}

fn instance_of_named_class(
    object_type: &str,
    declared: &str,
    class_edges: &BTreeMap<String, BTreeSet<String>>,
) -> bool {
    let mut visited = BTreeSet::from([object_type.to_owned()]);
    let mut pending = vec![object_type];
    while let Some(current) = pending.pop() {
        if current == declared {
            return true;
        }
        for next in class_edges.get(current).into_iter().flatten() {
            if visited.insert(next.clone()) {
                pending.push(next);
            }
        }
    }
    false
}

pub fn declared_cardinality_violations(
    ontology: &Ontology,
    objects: &[RuntimeObjectFact],
    relations: &[RuntimeRelationFact],
) -> Vec<RelationCardinalityViolation> {
    let mut violations = Vec::new();
    let mut bounds: BTreeMap<(String, String, String), (u32, u32)> = BTreeMap::new();
    for axiom in ontology.axioms() {
        let Axiom::SubClassOf { sub, sup } = axiom else {
            continue;
        };
        let ClassExpression::Class(source) = sub else {
            continue;
        };
        let (property, value, bound) = match sup {
            ClassExpression::ObjectMinCardinality {
                cardinality,
                property,
                value,
            } => (property, value, (*cardinality, u32::MAX)),
            ClassExpression::ObjectMaxCardinality {
                cardinality,
                property,
                value,
            } => (property, value, (0, *cardinality)),
            _ => continue,
        };
        let ClassExpression::Class(target) = value.as_ref() else {
            continue;
        };
        // Cardinality axioms outside the executable relation projection are left
        // to the mature reasoner; only exact ODL projections get a closed-world
        // admission check.
        if let Some(previous) =
            bounds.get_mut(&(source.0.clone(), property.0.clone(), target.0.clone()))
        {
            previous.0 = previous.0.max(bound.0);
            previous.1 = previous.1.min(bound.1);
        } else {
            bounds.insert(
                (source.0.clone(), property.0.clone(), target.0.clone()),
                bound,
            );
        }
    }

    let class_edges = named_class_edges(ontology);
    let instance_of = |object_type: &str, declared: &str| {
        instance_of_named_class(object_type, declared, &class_edges)
    };
    let types_by_id = objects
        .iter()
        .map(|object| (object.id.as_str(), object.object_type.as_str()))
        .collect::<BTreeMap<_, _>>();
    for ((source, property, target), (min, max)) in bounds {
        for subject in objects
            .iter()
            .filter(|object| instance_of(&object.object_type, &source))
            .map(|object| object.id.as_str())
        {
            let actual = relations
                .iter()
                .filter(|relation| {
                    relation.relation == property
                        && relation.source == subject
                        && types_by_id
                            .get(relation.target.as_str())
                            .is_some_and(|object_type| instance_of(object_type, &target))
                })
                .count();
            let actual = u32::try_from(actual).unwrap_or(u32::MAX);
            if actual < min || actual > max {
                violations.push(RelationCardinalityViolation {
                    individual: subject.to_owned(),
                    property: property.clone(),
                    target_class: target.clone(),
                    min,
                    max,
                    actual,
                });
            }
        }
    }
    violations
}

/// Check exact ODL scalar cardinalities against a closed world snapshot.
///
/// OWL correctly treats missing values as open-world unknowns; executable
/// admission needs the stricter snapshot-local answer for required/forbidden
/// fields.
pub fn declared_data_cardinality_violations(
    ontology: &Ontology,
    objects: &[RuntimeObjectFact],
) -> Vec<DataCardinalityViolation> {
    let mut violations = Vec::new();
    let mut bounds: BTreeMap<(String, String, Datatype), (u32, u32)> = BTreeMap::new();
    for axiom in ontology.axioms() {
        let Axiom::SubClassOf { sub, sup } = axiom else {
            continue;
        };
        let ClassExpression::Class(source) = sub else {
            continue;
        };
        let (property, datatype, bound) = match sup {
            ClassExpression::DataMinCardinality {
                cardinality,
                property,
                datatype,
            } => (property, datatype, (*cardinality, u32::MAX)),
            ClassExpression::DataMaxCardinality {
                cardinality,
                property,
                datatype,
            } => (property, datatype, (0, *cardinality)),
            _ => continue,
        };
        let key = (source.0.clone(), property.0.clone(), *datatype);
        if let Some(previous) = bounds.get_mut(&key) {
            previous.0 = previous.0.max(bound.0);
            previous.1 = previous.1.min(bound.1);
        } else {
            bounds.insert(key, bound);
        }
    }

    let class_edges = named_class_edges(ontology);
    for ((source, property, datatype), (min, max)) in bounds {
        for object in objects
            .iter()
            .filter(|object| instance_of_named_class(&object.object_type, &source, &class_edges))
        {
            let actual = u32::from(
                object
                    .data
                    .get(&property)
                    .is_some_and(|value| !value.is_null()),
            );
            if actual < min || actual > max {
                violations.push(DataCardinalityViolation {
                    individual: object.id.clone(),
                    property: property.clone(),
                    datatype,
                    min,
                    max,
                    actual,
                });
            }
        }
    }
    violations
}

fn validate_declared_cardinality_facts(
    ontology: &Ontology,
    objects: &[RuntimeObjectFact],
    relations: &[RuntimeRelationFact],
) -> Result<()> {
    let relation_violation = declared_cardinality_violations(ontology, objects, relations)
        .into_iter()
        .next()
        .map(|violation| {
            KnowledgeError::Invalid(format!(
                "individual `{}` requires {}..={} `{}` edges to `{}`, found {}",
                violation.individual,
                violation.min,
                violation.max,
                violation.property,
                violation.target_class,
                violation.actual
            ))
        });
    let data_violation = declared_data_cardinality_violations(ontology, objects)
        .into_iter()
        .next()
        .map(|violation| {
            KnowledgeError::Invalid(format!(
                "individual `{}` requires {}..={} values for `{}` ({:?}), found {}",
                violation.individual,
                violation.min,
                violation.max,
                violation.property,
                violation.datatype,
                violation.actual
            ))
        });
    match relation_violation.or(data_violation) {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

pub fn validate_canonical_ontology_with_world(
    ontology: &Ontology,
    objects: &[RuntimeObjectFact],
    relations: &[RuntimeRelationFact],
) -> Result<()> {
    validate_declared_cardinality_facts(ontology, objects, relations)?;
    let knowledge_base = runtime_world_to_knowledge(ontology, objects, relations)?;
    crate::data::validate_data_consistency(&knowledge_base)?;
    if Reasoner::new(&knowledge_base)?.is_consistent()? {
        Ok(())
    } else {
        Err(KnowledgeError::Invalid(
            "canonical ontology is inconsistent with the live world".to_owned(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oaip_abi::ir::{
        ActionDef, CreateEffect, DataPropertyHierarchy, DisjointUnionGroup,
        ObjectPropertyRestriction, ObjectRestrictionKind, ObjectType, PropertyDef, PropertyFacet,
        RelationDef, RelationHierarchy,
    };

    fn ir(actions: bool) -> OntologyIR {
        OntologyIR {
            relations: Vec::new(),
            disjoint_groups: Vec::new(),
            equivalent_groups: Vec::new(),
            disjoint_unions: Vec::new(),
            object_one_ofs: Vec::new(),
            relation_hierarchy: Vec::new(),
            disjoint_relation_groups: Vec::new(),
            equivalent_relation_groups: Vec::new(),
            data_property_hierarchy: Vec::new(),
            disjoint_data_property_groups: Vec::new(),
            equivalent_data_property_groups: Vec::new(),
            object_types: vec![ObjectType {
                name: "Customer".to_owned(),
                extends: Vec::new(),
                properties: vec![PropertyDef {
                    name: "name".to_owned(),
                    ty: ScalarType::String,
                    constraints: Vec::new(),
                }],
                actions: if actions {
                    vec![ActionDef {
                        name: "retain".to_owned(),
                        params: Vec::new(),
                        updates: Vec::new(),
                        creates: vec![CreateEffect {
                            object_type: "RetentionTask".to_owned(),
                            id: None,
                            data: Vec::new(),
                            link_edge: Some("needs_retention".to_owned()),
                        }],
                        policy: Default::default(),
                    }]
                } else {
                    Vec::new()
                },
                restrictions: Vec::new(),
                data_restrictions: Vec::new(),
            }],
        }
    }

    #[test]
    fn runtime_projects_classes_properties_and_action_relations() {
        let ontology = runtime_ir_to_knowledge(&ir(true));
        assert_eq!(7, ontology.len());
        assert!(ontology.axioms().contains(&Axiom::ObjectPropertyDomain {
            property: Term::new("needs_retention"),
            class: class("Customer"),
        }));
        assert!(ontology.axioms().contains(&Axiom::ObjectPropertyRange {
            property: Term::new("needs_retention"),
            class: class("RetentionTask"),
        }));
    }

    #[test]
    fn declared_extends_projects_into_reasoned_taxonomy() {
        let ir = oaip_abi::parse_odl(
            r#"
object Animal:
  properties:
    name: string
object Dog extends Animal:
  properties:
    breed: string
"#,
        )
        .unwrap();
        let schema = runtime_ir_to_knowledge(&ir);
        assert!(
            schema
                .axioms()
                .contains(&Axiom::subclass(class("Dog"), class("Animal")))
        );

        let fact = RuntimeObjectFact {
            id: "dog-1".to_owned(),
            object_type: "Dog".to_owned(),
            data: serde_json::json!({ "name": "Ada", "breed": "lab" }),
        };
        let knowledge_base = runtime_world_to_knowledge(&schema, &[fact], &[]).unwrap();
        let reasoner = Reasoner::new(&knowledge_base).unwrap();
        assert!(
            reasoner
                .entails_subclass(&Term::new("Dog"), &Term::new("Animal"))
                .unwrap()
        );
        assert!(
            reasoner
                .instances_of(&Term::new("Animal"))
                .unwrap()
                .contains(&Term::new("dog-1"))
        );
    }

    #[test]
    fn declared_class_groups_project_and_reason_fail_closed() {
        let ir = oaip_abi::parse_odl(
            r#"
object Animal:
  properties:
    name: string
object Dog extends Animal:
  properties:
    breed: string
object Cat extends Animal:
  properties:
    indoor: bool
object Pet:
  properties:
    nickname: string
object Companion:
  properties:
    owner_note: string
disjoint Dog, Cat
equivalent Pet, Companion
"#,
        )
        .unwrap();
        assert!(validate_runtime_ir(&ir).is_ok());
        let schema = runtime_ir_to_knowledge(&ir);
        assert!(
            schema
                .axioms()
                .contains(&Axiom::DisjointClasses(vec![class("Dog"), class("Cat")]))
        );
        assert!(schema.axioms().contains(&Axiom::EquivalentClasses(vec![
            class("Pet"),
            class("Companion")
        ])));

        let pet = RuntimeObjectFact {
            id: "pet-1".to_owned(),
            object_type: "Pet".to_owned(),
            data: serde_json::json!({ "nickname": "Ada" }),
        };
        let equivalent = runtime_world_to_knowledge(&schema, &[pet], &[]).unwrap();
        let reasoner = Reasoner::new(&equivalent).unwrap();
        assert!(
            reasoner
                .instances_of(&Term::new("Companion"))
                .unwrap()
                .contains(&Term::new("pet-1"))
        );

        let conflicting = Ontology::from_iter(schema.axioms().iter().cloned().chain([
            Axiom::ClassAssertion {
                class: class("Dog"),
                individual: Term::new("conflict"),
            },
            Axiom::ClassAssertion {
                class: class("Cat"),
                individual: Term::new("conflict"),
            },
        ]));
        let reasoner = Reasoner::new(&conflicting).unwrap();
        assert!(!reasoner.is_consistent().unwrap());
    }

    #[test]
    fn declared_disjoint_union_projects_and_reasons_covered_members() {
        let ir = oaip_abi::parse_odl(
            r#"
object Vehicle:
  properties:
    label: string
object Car:
  properties:
    wheels: int
object Boat:
  properties:
    draft: decimal
disjoint_union Vehicle = Car, Boat
"#,
        )
        .unwrap();
        assert!(validate_runtime_ir(&ir).is_ok());
        let schema = runtime_ir_to_knowledge(&ir);
        assert!(schema.axioms().contains(&Axiom::EquivalentClasses(vec![
            class("Vehicle"),
            ClassExpression::Union(vec![class("Car"), class("Boat")])
        ])));
        assert!(
            schema
                .axioms()
                .contains(&Axiom::DisjointClasses(vec![class("Car"), class("Boat")]))
        );

        let car = RuntimeObjectFact {
            id: "car-1".to_owned(),
            object_type: "Car".to_owned(),
            data: serde_json::json!({ "wheels": 4 }),
        };
        let world = runtime_world_to_knowledge(&schema, &[car], &[]).unwrap();
        let reasoner = Reasoner::new(&world).unwrap();
        assert!(
            reasoner
                .instances_of(&Term::new("Vehicle"))
                .unwrap()
                .contains(&Term::new("car-1"))
        );

        let conflict_axioms = schema.axioms().iter().cloned().chain([
            Axiom::ClassAssertion {
                class: class("Car"),
                individual: Term::new("car-1"),
            },
            Axiom::ClassAssertion {
                class: class("Boat"),
                individual: Term::new("car-1"),
            },
        ]);
        let conflicting = Ontology::from_iter(conflict_axioms);
        let reasoner = Reasoner::new(&conflicting).unwrap();
        assert!(!reasoner.is_consistent().unwrap());
    }

    #[test]
    fn declared_object_one_of_projects_and_reasons_nominals() {
        let ir = oaip_abi::parse_odl(
            r#"
object Planet:
  properties:
    name: string

one_of Planet = Mercury, Venus, Earth
"#,
        )
        .unwrap();
        assert!(validate_runtime_ir(&ir).is_ok());
        let schema = runtime_ir_to_knowledge(&ir);
        assert!(schema.axioms().contains(&Axiom::EquivalentClasses(vec![
            class("Planet"),
            ClassExpression::ObjectOneOf(vec![
                Term::new("Mercury"),
                Term::new("Venus"),
                Term::new("Earth"),
            ]),
        ])));

        // A declared nominal is inferred into the enumerated class.
        let mercury = RuntimeObjectFact {
            id: "mercury".to_owned(),
            object_type: "Planet".to_owned(),
            data: serde_json::json!({ "name": "Mercury" }),
        };
        let world = runtime_world_to_knowledge(&schema, &[mercury], &[]).unwrap();
        assert!(Reasoner::new(&world).unwrap().is_consistent().unwrap());

        // A non-nominal individual asserted as Planet remains satisfiable under
        // open-world semantics without a distinctness assumption, but the
        // enumeration itself is correctly projected and reasoned about.
        let reasoner = Reasoner::new(&world).unwrap();
        let members = reasoner
            .instances_of(&Term::new("Planet"))
            .unwrap()
            .contains(&Term::new("mercury"));
        assert!(members);
    }

    #[test]
    fn runtime_validation_rejects_invalid_class_groups() {
        let mut ir = ir(false);
        ir.object_types.push(ObjectType {
            name: "Dog".to_owned(),
            extends: Vec::new(),
            properties: vec![PropertyDef {
                name: "breed".to_owned(),
                ty: ScalarType::String,
                constraints: Vec::new(),
            }],
            actions: Vec::new(),
            restrictions: Vec::new(),
            data_restrictions: Vec::new(),
        });
        ir.disjoint_groups = vec![vec!["Dog".to_owned(), "Cat".to_owned()]];
        assert!(matches!(
            validate_runtime_ir(&ir),
            Err(KnowledgeError::Invalid(message)) if message.contains("undeclared")
        ));

        ir.disjoint_groups = vec![vec!["Dog".to_owned(), "Dog".to_owned()]];
        assert!(matches!(
            validate_runtime_ir(&ir),
            Err(KnowledgeError::Invalid(message)) if message.contains("repeats")
        ));

        ir.disjoint_groups = Vec::new();
        ir.disjoint_unions = vec![DisjointUnionGroup {
            class: "Customer".to_owned(),
            members: vec!["Dog".to_owned(), "Cat".to_owned()],
        }];
        assert!(matches!(
            validate_runtime_ir(&ir),
            Err(KnowledgeError::Invalid(message)) if message.contains("undeclared")
        ));

        ir.disjoint_unions = Vec::new();
        ir.relations.push(RelationDef {
            name: "child".to_owned(),
            source: "Customer".to_owned(),
            target: "Dog".to_owned(),
            ..RelationDef::default()
        });
        ir.object_types[0]
            .restrictions
            .push(ObjectPropertyRestriction {
                relation: "child".to_owned(),
                kind: ObjectRestrictionKind::Some {
                    class: "Cat".to_owned(),
                },
            });
        assert!(matches!(
            validate_runtime_ir(&ir),
            Err(KnowledgeError::Invalid(message)) if message.contains("undeclared class")
        ));

        ir.object_types[0].restrictions[0] = ObjectPropertyRestriction {
            relation: "child".to_owned(),
            kind: ObjectRestrictionKind::Value {
                individual: "  ".to_owned(),
            },
        };
        assert!(matches!(
            validate_runtime_ir(&ir),
            Err(KnowledgeError::Invalid(message)) if message.contains("individual is empty")
        ));
    }

    #[test]
    fn declared_object_restrictions_project_and_reason_relationships() {
        let ir = oaip_abi::parse_odl(
            r#"
object Person:
  properties:
    name: string

object Robot:
  properties:
    serial: string

disjoint Person, Robot

object Parent extends Person:
  properties:
    note: string
  restrictions:
    child: some Person

object BigFamily extends Parent:
  properties:
    note: string
  restrictions:
    child: min 2 Person

object SafeContact extends Person:
  properties:
    note: string
  restrictions:
    contact: only Person

object AliceWatcher extends Person:
  properties:
    note: string
  restrictions:
    guardian: value alice

relation child:
  source: Person
  target: Person

relation contact:
  source: Person
  target: Person

relation guardian:
  source: Person
  target: Person
"#,
        )
        .unwrap();
        assert!(validate_runtime_ir(&ir).is_ok());
        let schema = runtime_ir_to_knowledge(&ir);
        assert!(schema.axioms().contains(&Axiom::subclass(
            class("Parent"),
            ClassExpression::some_values_from("child", class("Person"))
        )));
        assert!(schema.axioms().contains(&Axiom::subclass(
            class("BigFamily"),
            ClassExpression::min_cardinality(2, "child", class("Person"))
        )));
        assert!(schema.axioms().contains(&Axiom::subclass(
            class("SafeContact"),
            ClassExpression::all_values_from("contact", class("Person"))
        )));
        assert!(schema.axioms().contains(&Axiom::subclass(
            class("AliceWatcher"),
            ClassExpression::has_value("guardian", "alice")
        )));

        let object = |id: &str, ty: &str| RuntimeObjectFact {
            id: id.to_owned(),
            object_type: ty.to_owned(),
            data: serde_json::json!({ "name": id }),
        };
        let objects = [
            object("parent-1", "Parent"),
            object("person-1", "Person"),
            object("contact-1", "SafeContact"),
            object("watcher-1", "AliceWatcher"),
        ];
        let relations = [
            RuntimeRelationFact {
                source: "parent-1".to_owned(),
                relation: "child".to_owned(),
                target: "person-1".to_owned(),
            },
            RuntimeRelationFact {
                source: "watcher-1".to_owned(),
                relation: "guardian".to_owned(),
                target: "alice".to_owned(),
            },
        ];
        let world = runtime_world_to_knowledge(&schema, &objects, &relations).unwrap();
        let reasoner = Reasoner::new(&world).unwrap();
        assert!(
            reasoner
                .instances_of(&Term::new("Parent"))
                .unwrap()
                .contains(&Term::new("parent-1"))
        );
        assert!(
            reasoner
                .instances_of(&Term::new("AliceWatcher"))
                .unwrap()
                .contains(&Term::new("watcher-1"))
        );

        // The mature tableau sees an `only` breach as inconsistency when the
        // forbidden edge reaches a class disjoint from the declared filler.
        let invalid_edge = Ontology::from_iter(schema.axioms().iter().cloned().chain([
            Axiom::ClassAssertion {
                class: class("SafeContact"),
                individual: Term::new("contact-1"),
            },
            Axiom::ClassAssertion {
                class: class("Robot"),
                individual: Term::new("robot-1"),
            },
            Axiom::ObjectPropertyAssertion {
                property: Term::new("contact"),
                subject: Term::new("contact-1"),
                object: Term::new("robot-1"),
            },
        ]));
        assert!(
            !Reasoner::new(&invalid_edge)
                .unwrap()
                .is_consistent()
                .unwrap()
        );
    }

    #[test]
    fn declared_data_restrictions_project_and_gate_world_facts() {
        let ir = oaip_abi::parse_odl(
            r#"
object Person:
  properties:
    name: string
    age: int

object TaggedPerson extends Person:
  properties:
    note: string
  data_restrictions:
    note: some string

object StrictPerson extends Person:
  properties:
    note: string
  data_restrictions:
    note: only string

object ActivePerson extends Person:
  properties:
    status: string
  data_restrictions:
    status: value "active"
"#,
        )
        .unwrap();
        assert!(validate_runtime_ir(&ir).is_ok());
        let schema = runtime_ir_to_knowledge(&ir);
        assert!(schema.axioms().contains(&Axiom::subclass(
            class("TaggedPerson"),
            ClassExpression::some_data("note", Datatype::String)
        )));
        assert!(schema.axioms().contains(&Axiom::subclass(
            class("StrictPerson"),
            ClassExpression::all_data("note", Datatype::String)
        )));
        assert!(schema.axioms().contains(&Axiom::subclass(
            class("ActivePerson"),
            ClassExpression::has_data("status", Literal::String("active".to_owned()))
        )));

        let object = |id: &str, ty: &str| RuntimeObjectFact {
            id: id.to_owned(),
            object_type: ty.to_owned(),
            data: serde_json::json!({ "name": id, "age": 30 }),
        };
        let objects = [
            {
                let mut o = object("tagged-1", "TaggedPerson");
                o.data["note"] = serde_json::json!("hello");
                o
            },
            object("strict-1", "StrictPerson"),
            {
                let mut o = object("active-1", "ActivePerson");
                o.data["status"] = serde_json::json!("active");
                o
            },
        ];
        let world = runtime_world_to_knowledge(&schema, &objects, &[]).unwrap();
        assert!(validate_canonical_ontology_with_world(&schema, &objects, &[]).is_ok());
        let reasoner = Reasoner::new(&world).unwrap();
        for id in ["tagged-1", "strict-1", "active-1"] {
            assert!(
                reasoner
                    .instances_of(&Term::new("Person"))
                    .unwrap()
                    .contains(&Term::new(id)),
                "{id} should be inferred as a Person"
            );
        }

        // The parser rejects an undeclared property reference (fail-closed).
        let bad = oaip_abi::parse_odl(
            r#"
object Person:
  properties:
    name: string
  data_restrictions:
    missing: some string
"#,
        );
        assert!(bad.is_err());

        // The parser rejects a datatype mismatch.
        let mismatched = oaip_abi::parse_odl(
            r#"
object Person:
  properties:
    name: string
  data_restrictions:
    name: some int
"#,
        );
        assert!(mismatched.is_err());
    }

    #[test]
    fn runtime_validation_rejects_undeclared_and_cyclic_extends() {
        let mut ir = ir(false);
        ir.object_types.push(ObjectType {
            name: "Dog".to_owned(),
            extends: vec!["Animal".to_owned()],
            properties: vec![PropertyDef {
                name: "breed".to_owned(),
                ty: ScalarType::String,
                constraints: Vec::new(),
            }],
            actions: Vec::new(),
            restrictions: Vec::new(),
            data_restrictions: Vec::new(),
        });
        assert!(validate_runtime_ir(&ir).is_err());

        ir.object_types[0].name = "Animal".to_owned();
        ir.object_types[0].extends = vec!["Dog".to_owned()];
        assert!(matches!(
            validate_runtime_ir(&ir),
            Err(KnowledgeError::Invalid(message)) if message.contains("继承关系形成环")
        ));
    }

    #[test]
    fn declared_float_range_normalizes_integral_json_numbers() {
        let mut ir = ir(false);
        ir.object_types[0].properties.push(PropertyDef {
            name: "score".to_owned(),
            ty: ScalarType::Float,
            constraints: Vec::new(),
        });
        let schema = runtime_ir_to_knowledge(&ir);
        let fact = RuntimeObjectFact {
            id: "c1".to_owned(),
            object_type: "Customer".to_owned(),
            data: serde_json::json!({ "name": "Grace", "score": 30 }),
        };
        let knowledge_base = runtime_world_to_knowledge(&schema, &[fact], &[]).unwrap();
        let score = knowledge_base
            .axioms()
            .iter()
            .find_map(|axiom| match axiom {
                Axiom::DataPropertyAssertion {
                    property, value, ..
                } if property.as_str() == "score" => Some(value.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(score, Literal::Double(30.0));
        assert!(
            Reasoner::new(&knowledge_base)
                .unwrap()
                .is_consistent()
                .unwrap()
        );
    }

    #[test]
    fn declared_date_time_range_normalizes_and_rejects_invalid_json() {
        let mut ir = ir(false);
        ir.object_types[0].properties.push(PropertyDef {
            name: "occurred_at".to_owned(),
            ty: ScalarType::DateTime,
            constraints: Vec::new(),
        });
        let schema = runtime_ir_to_knowledge(&ir);
        let fact = |occurred_at: &str| RuntimeObjectFact {
            id: "c1".to_owned(),
            object_type: "Customer".to_owned(),
            data: serde_json::json!({ "name": "Grace", "occurred_at": occurred_at }),
        };

        let knowledge_base =
            runtime_world_to_knowledge(&schema, &[fact("2026-08-23T08:00:00+08:00")], &[]).unwrap();
        let value = knowledge_base
            .axioms()
            .iter()
            .find_map(|axiom| match axiom {
                Axiom::DataPropertyAssertion {
                    property,
                    value: Literal::DateTime(value),
                    ..
                } if property.as_str() == "occurred_at" => Some(value.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!("2026-08-23T00:00:00Z", value);
        assert!(
            Reasoner::new(&knowledge_base)
                .unwrap()
                .is_consistent()
                .unwrap()
        );
        assert!(runtime_world_to_knowledge(&schema, &[fact("not-a-date")], &[]).is_err());
    }

    #[test]
    fn world_facts_participate_in_canonical_consistency_gate() {
        let mut ontology = Ontology::new();
        for name in ["Customer", "Order"] {
            ontology.push(Axiom::subclass(class(name), ClassExpression::Thing));
        }
        // The world gate must reject a schema that entails an existing object
        // has no allowable interpretation.
        ontology.push(Axiom::subclass(class("Customer"), ClassExpression::Nothing));

        let objects = vec![
            RuntimeObjectFact {
                id: "c1".to_owned(),
                object_type: "Customer".to_owned(),
                data: serde_json::json!({}),
            },
            RuntimeObjectFact {
                id: "o1".to_owned(),
                object_type: "Order".to_owned(),
                data: serde_json::json!({}),
            },
        ];
        assert!(Reasoner::new(&ontology).unwrap().is_consistent().unwrap());
        assert!(validate_canonical_ontology_with_world(&ontology, &objects, &[]).is_err());
    }

    #[test]
    fn declared_decimal_range_preserves_exact_lexical_values() {
        let mut ir = ir(false);
        ir.object_types[0].properties.push(PropertyDef {
            name: "balance".to_owned(),
            ty: ScalarType::Decimal,
            constraints: Vec::new(),
        });
        let schema = runtime_ir_to_knowledge(&ir);
        let fact = |balance: serde_json::Value| RuntimeObjectFact {
            id: "c1".to_owned(),
            object_type: "Customer".to_owned(),
            data: serde_json::json!({ "name": "Grace", "balance": balance }),
        };

        for (input, expected) in [
            (serde_json::Value::String("+0001.2300".into()), "1.23"),
            (serde_json::json!(1), "1"),
        ] {
            let knowledge_base = runtime_world_to_knowledge(&schema, &[fact(input)], &[]).unwrap();
            let value = knowledge_base
                .axioms()
                .iter()
                .find_map(|axiom| match axiom {
                    Axiom::DataPropertyAssertion {
                        property,
                        value: Literal::Decimal(value),
                        ..
                    } if property.as_str() == "balance" => Some(value.clone()),
                    _ => None,
                })
                .unwrap();
            assert_eq!(expected, value.as_str());
            assert!(
                Reasoner::new(&knowledge_base)
                    .unwrap()
                    .is_consistent()
                    .unwrap()
            );
        }

        for invalid in [
            serde_json::json!(1.23),
            serde_json::Value::String("1e3".into()),
        ] {
            assert!(runtime_world_to_knowledge(&schema, &[fact(invalid)], &[]).is_err());
        }
    }

    #[test]
    fn declared_property_constraints_project_and_gate_world_facts() {
        let mut ir = ir(false);
        ir.object_types[0].properties.push(PropertyDef {
            name: "code".to_owned(),
            ty: ScalarType::String,
            constraints: vec![
                PropertyFacet::MinLength("2".to_owned()),
                PropertyFacet::Pattern("[A-Z]{2}[0-9]{4}".to_owned()),
            ],
        });
        let schema = runtime_ir_to_knowledge(&ir);
        assert!(validate_runtime_ir(&ir).is_ok());
        assert!(schema.axioms().iter().any(|axiom| matches!(
            axiom,
            Axiom::DataPropertyRangeRestriction { property, datatype: Datatype::String, facets }
                if property.as_str() == "code" && facets.len() == 2
        )));

        let fact = |code: &str| RuntimeObjectFact {
            id: "c1".to_owned(),
            object_type: "Customer".to_owned(),
            data: serde_json::json!({ "name": "Grace", "code": code }),
        };
        let valid = runtime_world_to_knowledge(&schema, &[fact("AB1234")], &[]).unwrap();
        assert!(Reasoner::new(&valid).unwrap().is_consistent().unwrap());
        let invalid = runtime_world_to_knowledge(&schema, &[fact("ab1234")], &[]).unwrap();
        assert!(!Reasoner::new(&invalid).unwrap().is_consistent().unwrap());
    }

    #[test]
    fn nested_odl_data_ranges_project_and_gate_world_facts() {
        let schema = oaip_abi::parse_odl(
            r#"object Customer:
  properties:
    code: string range(intersection(datatype,union(restriction(pattern "[A-Z]{3}-[0-9]{4}"),one_of [ADMIN,GUEST])))
"#,
        )
        .unwrap();
        validate_runtime_ir(&schema).unwrap();
        let schema = runtime_ir_to_knowledge(&schema);
        let range = schema.axioms().iter().find_map(|axiom| match axiom {
            Axiom::DataPropertyDataRange { property, range } if property.as_str() == "code" => {
                Some(range.clone())
            }
            _ => None,
        });
        assert_eq!(
            Some(ModelDataRange::Intersection(vec![
                ModelDataRange::Datatype(Datatype::String),
                ModelDataRange::Union(vec![
                    ModelDataRange::Restriction {
                        datatype: Datatype::String,
                        facets: vec![FacetValue {
                            facet: DataFacet::Pattern,
                            value: Literal::String("[A-Z]{3}-[0-9]{4}".into()),
                        }],
                    },
                    ModelDataRange::OneOf(vec![
                        Literal::String("ADMIN".into()),
                        Literal::String("GUEST".into()),
                    ]),
                ]),
            ])),
            range
        );

        let fact = |code: &str| RuntimeObjectFact {
            id: "customer-1".to_owned(),
            object_type: "Customer".to_owned(),
            data: serde_json::json!({ "code": code }),
        };
        for accepted in ["ADMIN", "GUEST", "ABC-1234"] {
            let world = runtime_world_to_knowledge(&schema, &[fact(accepted)], &[]).unwrap();
            assert!(Reasoner::new(&world).unwrap().is_consistent().unwrap());
            assert!(
                validate_canonical_ontology_with_world(&schema, &[fact(accepted)], &[]).is_ok()
            );
        }
        for rejected in ["admin", "ABC-123", "administrator"] {
            let world = runtime_world_to_knowledge(&schema, &[fact(rejected)], &[]).unwrap();
            assert!(!Reasoner::new(&world).unwrap().is_consistent().unwrap());
            assert!(
                validate_canonical_ontology_with_world(&schema, &[fact(rejected)], &[]).is_err()
            );
        }
    }

    #[test]
    fn odl_cross_datatype_union_projects_and_gates_world_facts() {
        let schema = oaip_abi::parse_odl(
            r#"object Customer:
  properties:
    contact: string range(union(restriction(pattern "[^@]+@[^@]+"),datatype(int)))
"#,
        )
        .unwrap();
        validate_runtime_ir(&schema).unwrap();
        let schema = runtime_ir_to_knowledge(&schema);

        assert!(
            !schema.axioms().iter().any(|axiom| matches!(
                axiom,
                Axiom::DataPropertyRange { property, .. } if property.as_str() == "contact"
            )),
            "a cross-datatype union must not also narrow the property to its leading type"
        );
        assert!(schema.axioms().iter().any(|axiom| matches!(
            axiom,
            Axiom::DataPropertyDataRange { property, range }
                if property.as_str() == "contact"
                    && matches!(range, ModelDataRange::Union(branches)
                        if matches!(branches.as_slice(), [
                            ModelDataRange::Restriction { datatype: Datatype::String, .. },
                            ModelDataRange::Datatype(Datatype::Integer),
                        ])
            )
        )));

        let fact = |contact: serde_json::Value| RuntimeObjectFact {
            id: "customer-1".to_owned(),
            object_type: "Customer".to_owned(),
            data: serde_json::json!({ "contact": contact }),
        };
        for accepted in [
            serde_json::Value::String("user@example.com".to_owned()),
            serde_json::json!(42),
        ] {
            let world =
                runtime_world_to_knowledge(&schema, &[fact(accepted.clone())], &[]).unwrap();
            assert!(Reasoner::new(&world).unwrap().is_consistent().unwrap());
            assert!(
                validate_canonical_ontology_with_world(&schema, &[fact(accepted)], &[]).is_ok()
            );
        }

        let rejected = fact(serde_json::json!(true));
        assert!(runtime_world_to_knowledge(&schema, &[rejected], &[]).is_err());

        // The governance boundary rejects a hand-built IR that omits the
        // declared leading datatype, even when the parser is bypassed.
        let mut invalid = oaip_abi::parse_odl(
            r#"object Customer:
  properties:
    contact: string range(union(datatype,datatype(int)))
"#,
        )
        .unwrap();
        invalid.object_types[0].properties[0].constraints[0] =
            PropertyFacet::Range(PropertyDataRange::Union(vec![
                PropertyDataRange::Base(ScalarType::Int),
                PropertyDataRange::Base(ScalarType::Float),
            ]));
        assert!(validate_runtime_ir(&invalid).is_err());
    }

    #[test]
    fn odl_complement_range_projects_and_gates_world_facts() {
        let schema = oaip_abi::parse_odl(
            r#"object Customer:
  properties:
    code: string range(complement(restriction(pattern "[A-Z]{3}-[0-9]{4}")))
"#,
        )
        .unwrap();
        validate_runtime_ir(&schema).unwrap();
        let schema = runtime_ir_to_knowledge(&schema);
        let range = schema.axioms().iter().find_map(|axiom| match axiom {
            Axiom::DataPropertyDataRange { property, range } if property.as_str() == "code" => {
                Some(range.clone())
            }
            _ => None,
        });
        assert_eq!(
            Some(ModelDataRange::Complement(Box::new(
                ModelDataRange::Restriction {
                    datatype: Datatype::String,
                    facets: vec![FacetValue {
                        facet: DataFacet::Pattern,
                        value: Literal::String("[A-Z]{3}-[0-9]{4}".into()),
                    }],
                }
            ))),
            range
        );

        let fact = |code: &str| RuntimeObjectFact {
            id: "customer-1".to_owned(),
            object_type: "Customer".to_owned(),
            data: serde_json::json!({ "code": code }),
        };
        let admitted = runtime_world_to_knowledge(&schema, &[fact("admin")], &[]).unwrap();
        assert!(Reasoner::new(&admitted).unwrap().is_consistent().unwrap());
        assert!(validate_canonical_ontology_with_world(&schema, &[fact("admin")], &[]).is_ok());

        for rejected in ["ABC-1234", "AAA-0000"] {
            let world = runtime_world_to_knowledge(&schema, &[fact(rejected)], &[]).unwrap();
            assert!(
                !Reasoner::new(&world).unwrap().is_consistent().unwrap(),
                "unexpected admission: {rejected}"
            );
            assert!(
                validate_canonical_ontology_with_world(&schema, &[fact(rejected)], &[]).is_err()
            );
        }
    }

    #[test]
    fn declared_date_range_normalizes_and_rejects_non_calendar_json() {
        let mut ir = ir(false);
        ir.object_types[0].properties.push(PropertyDef {
            name: "birthday".to_owned(),
            ty: ScalarType::Date,
            constraints: Vec::new(),
        });
        let schema = runtime_ir_to_knowledge(&ir);
        assert_eq!(Datatype::Date, scalar_datatype(ScalarType::Date));
        let fact = |birthday: &str| RuntimeObjectFact {
            id: "c1".to_owned(),
            object_type: "Customer".to_owned(),
            data: serde_json::json!({ "name": "Grace", "birthday": birthday }),
        };

        let knowledge_base =
            runtime_world_to_knowledge(&schema, &[fact("2024-02-29")], &[]).unwrap();
        let value = knowledge_base
            .axioms()
            .iter()
            .find_map(|axiom| match axiom {
                Axiom::DataPropertyAssertion {
                    property,
                    value: Literal::Date(value),
                    ..
                } if property.as_str() == "birthday" => Some(value.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!("2024-02-29", value);
        assert!(
            Reasoner::new(&knowledge_base)
                .unwrap()
                .is_consistent()
                .unwrap()
        );
        for invalid in ["2026-08-23T00:00:00+08:00", "2026-8-23", "not-a-date"] {
            assert!(runtime_world_to_knowledge(&schema, &[fact(invalid)], &[]).is_err());
        }
    }

    #[test]
    fn declared_relations_share_the_action_edge_projection() {
        let mut schema = ir(false);
        schema.relations.push(RelationDef {
            name: "placed".to_owned(),
            source: "Customer".to_owned(),
            target: "Customer".to_owned(),
            ..RelationDef::default()
        });
        let ontology = runtime_ir_to_knowledge(&schema);
        assert_eq!(6, ontology.len());
        assert!(validate_runtime_ir(&schema).is_ok());
        assert!(ontology.axioms().contains(&Axiom::ObjectPropertyDomain {
            property: Term::new("placed"),
            class: class("Customer"),
        }));

        let mut conflicting = ir(true);
        conflicting.relations.push(RelationDef {
            name: "needs_retention".to_owned(),
            source: "Customer".to_owned(),
            target: "Customer".to_owned(),
            ..RelationDef::default()
        });
        assert!(validate_runtime_ir(&conflicting).is_err());
    }

    #[test]
    fn declared_relation_cardinalities_project_into_the_canonical_tbox() {
        let mut schema = ir(false);
        schema.relations.push(RelationDef {
            name: "placed".to_owned(),
            source: "Customer".to_owned(),
            target: "Customer".to_owned(),
            min_cardinality: Some(1),
            max_cardinality: Some(2),
            ..RelationDef::default()
        });
        assert!(validate_runtime_ir(&schema).is_ok());
        let ontology = runtime_ir_to_knowledge(&schema);
        let source = class("Customer");
        let target = Box::new(class("Customer"));
        let property = Term::new("placed");
        assert!(ontology.axioms().contains(&Axiom::subclass(
            source.clone(),
            ClassExpression::ObjectMinCardinality {
                cardinality: 1,
                property: property.clone(),
                value: target.clone(),
            }
        )));
        assert!(ontology.axioms().contains(&Axiom::subclass(
            source,
            ClassExpression::ObjectMaxCardinality {
                cardinality: 2,
                property,
                value: target,
            }
        )));

        let mut invalid = schema;
        invalid.relations[0].min_cardinality = Some(3);
        invalid.relations[0].max_cardinality = Some(2);
        assert!(validate_runtime_ir(&invalid).is_err());
    }

    #[test]
    fn declared_relation_chains_project_into_the_canonical_tbox() {
        let mut schema = ir(false);
        schema.relations = vec![
            RelationDef {
                name: "has_parent".to_owned(),
                source: "Customer".to_owned(),
                target: "Customer".to_owned(),
                ..RelationDef::default()
            },
            RelationDef {
                name: "has_grandparent".to_owned(),
                source: "Customer".to_owned(),
                target: "Customer".to_owned(),
                composed_of: vec!["has_parent".to_owned(), "has_parent".to_owned()],
                ..RelationDef::default()
            },
        ];
        assert!(validate_runtime_ir(&schema).is_ok());
        let ontology = runtime_ir_to_knowledge(&schema);
        assert!(
            ontology
                .axioms()
                .contains(&Axiom::SubObjectPropertyChainOf {
                    properties: vec![Term::new("has_parent"), Term::new("has_parent")],
                    super_property: Term::new("has_grandparent"),
                })
        );

        let mut invalid = schema;
        invalid.relations[1].composed_of = vec!["missing".to_owned()];
        assert!(validate_runtime_ir(&invalid).is_err());
    }

    #[test]
    fn declared_relation_property_groups_reason_and_gate_world_facts() {
        let relation = |name: &str| RelationDef {
            name: name.to_owned(),
            source: "Customer".to_owned(),
            target: "Customer".to_owned(),
            ..RelationDef::default()
        };
        let mut schema = ir(false);
        schema.relations = vec![
            relation("biological_parent"),
            relation("legal_parent"),
            relation("parent"),
            relation("guardian"),
        ];
        schema.relation_hierarchy = vec![RelationHierarchy {
            sub_relation: "biological_parent".to_owned(),
            super_relation: "parent".to_owned(),
        }];
        schema.disjoint_relation_groups = vec![vec![
            "biological_parent".to_owned(),
            "legal_parent".to_owned(),
        ]];
        schema.equivalent_relation_groups = vec![vec!["parent".to_owned(), "guardian".to_owned()]];

        assert!(validate_runtime_ir(&schema).is_ok());
        let ontology = runtime_ir_to_knowledge(&schema);
        assert!(ontology.axioms().contains(&Axiom::SubObjectPropertyOf {
            sub: Term::new("biological_parent"),
            sup: Term::new("parent"),
        }));
        assert!(
            ontology
                .axioms()
                .contains(&Axiom::DisjointObjectProperties(vec![
                    Term::new("biological_parent"),
                    Term::new("legal_parent"),
                ]))
        );
        for (sub, sup) in [("parent", "guardian"), ("guardian", "parent")] {
            assert!(ontology.axioms().contains(&Axiom::SubObjectPropertyOf {
                sub: Term::new(sub),
                sup: Term::new(sup),
            }));
        }

        let objects = [
            RuntimeObjectFact {
                id: "person-a".to_owned(),
                object_type: "Customer".to_owned(),
                data: serde_json::json!({ "name": "Alice" }),
            },
            RuntimeObjectFact {
                id: "person-b".to_owned(),
                object_type: "Customer".to_owned(),
                data: serde_json::json!({ "name": "Bob" }),
            },
        ];
        let biological = RuntimeRelationFact {
            source: "person-a".to_owned(),
            relation: "biological_parent".to_owned(),
            target: "person-b".to_owned(),
        };
        let valid_world =
            runtime_world_to_knowledge(&ontology, &objects, std::slice::from_ref(&biological))
                .unwrap();
        let reasoner = Reasoner::new(&valid_world).unwrap();
        assert!(reasoner.is_consistent().unwrap());
        assert!(
            reasoner
                .entails_object_property(
                    &Term::new("parent"),
                    &Term::new("person-a"),
                    &Term::new("person-b")
                )
                .unwrap()
        );
        assert!(
            reasoner
                .entails_object_property(
                    &Term::new("guardian"),
                    &Term::new("person-a"),
                    &Term::new("person-b")
                )
                .unwrap()
        );

        let mut invalid = schema.clone();
        invalid.equivalent_relation_groups = vec![vec!["parent".to_owned(), "missing".to_owned()]];
        assert!(validate_runtime_ir(&invalid).is_err());

        let conflicting = [
            biological,
            RuntimeRelationFact {
                source: "person-a".to_owned(),
                relation: "legal_parent".to_owned(),
                target: "person-b".to_owned(),
            },
        ];
        assert!(validate_canonical_ontology_with_world(&ontology, &objects, &conflicting).is_err());
    }

    #[test]
    fn declared_data_property_groups_reason_and_gate_world_facts() {
        let mut schema = ir(false);
        schema.object_types[0].properties.extend([
            PropertyDef {
                name: "email".to_owned(),
                ty: ScalarType::String,
                constraints: Vec::new(),
            },
            PropertyDef {
                name: "work_email".to_owned(),
                ty: ScalarType::String,
                constraints: Vec::new(),
            },
            PropertyDef {
                name: "mobile".to_owned(),
                ty: ScalarType::String,
                constraints: Vec::new(),
            },
            PropertyDef {
                name: "landline".to_owned(),
                ty: ScalarType::String,
                constraints: Vec::new(),
            },
            PropertyDef {
                name: "primary_contact".to_owned(),
                ty: ScalarType::String,
                constraints: Vec::new(),
            },
            PropertyDef {
                name: "contact".to_owned(),
                ty: ScalarType::String,
                constraints: Vec::new(),
            },
        ]);
        schema.data_property_hierarchy = vec![DataPropertyHierarchy {
            sub_property: "work_email".to_owned(),
            super_property: "email".to_owned(),
        }];
        schema.disjoint_data_property_groups =
            vec![vec!["mobile".to_owned(), "landline".to_owned()]];
        schema.equivalent_data_property_groups =
            vec![vec!["primary_contact".to_owned(), "contact".to_owned()]];

        assert!(validate_runtime_ir(&schema).is_ok());
        let ontology = runtime_ir_to_knowledge(&schema);
        assert!(ontology.axioms().contains(&Axiom::SubDataPropertyOf {
            sub: Term::new("work_email"),
            sup: Term::new("email"),
        }));
        assert!(
            ontology
                .axioms()
                .contains(&Axiom::DisjointDataProperties(vec![
                    Term::new("mobile"),
                    Term::new("landline"),
                ]))
        );
        for (sub, sup) in [
            ("primary_contact", "contact"),
            ("contact", "primary_contact"),
        ] {
            assert!(ontology.axioms().contains(&Axiom::SubDataPropertyOf {
                sub: Term::new(sub),
                sup: Term::new(sup),
            }));
        }

        let mut invalid_equivalent = schema.clone();
        invalid_equivalent.equivalent_data_property_groups =
            vec![vec!["contact".to_owned(), "missing".to_owned()]];
        assert!(validate_runtime_ir(&invalid_equivalent).is_err());

        let mut duplicate_equivalent = schema.clone();
        duplicate_equivalent.equivalent_data_property_groups = vec![
            vec!["contact".to_owned(), "primary_contact".to_owned()],
            vec!["primary_contact".to_owned(), "contact".to_owned()],
        ];
        assert!(validate_runtime_ir(&duplicate_equivalent).is_err());

        let customer = |data: serde_json::Value| RuntimeObjectFact {
            id: "customer-1".to_owned(),
            object_type: "Customer".to_owned(),
            data,
        };
        let valid_world = runtime_world_to_knowledge(
            &ontology,
            &[customer(serde_json::json!({
                "name": "Alice",
                "work_email": "alice@work.test",
                "mobile": "+8610000000000",
                "primary_contact": "alice@example.test",
            }))],
            &[],
        )
        .unwrap();
        let reasoner = Reasoner::new(&valid_world).unwrap();
        assert!(reasoner.is_consistent().unwrap());
        assert!(
            reasoner
                .entails_data_property(
                    &Term::new("email"),
                    &Term::new("customer-1"),
                    &Literal::String("alice@work.test".into())
                )
                .unwrap()
        );
        assert!(
            reasoner
                .entails_data_property(
                    &Term::new("contact"),
                    &Term::new("customer-1"),
                    &Literal::String("alice@example.test".into())
                )
                .unwrap()
        );

        let conflicting_world = runtime_world_to_knowledge(
            &ontology,
            &[customer(serde_json::json!({
                "name": "Alice",
                "mobile": "+8610000000000",
                "landline": "+8620000000000",
            }))],
            &[],
        )
        .unwrap();
        assert!(
            !Reasoner::new(&conflicting_world)
                .unwrap()
                .is_consistent()
                .unwrap()
        );
        assert!(
            validate_canonical_ontology_with_world(
                &ontology,
                &[customer(serde_json::json!({
                    "name": "Alice",
                    "mobile": "+8610000000000",
                    "landline": "+8620000000000",
                }))],
                &[],
            )
            .is_err()
        );
    }

    #[test]
    fn declared_relation_cardinalities_gate_inherited_instances() {
        let ir = oaip_abi::parse_odl(
            r#"
object Animal:
  properties:
    name: string
object Dog extends Animal:
  properties:
    breed: string
relation companion_of:
  source: Animal
  target: Animal
  min_cardinality: 1
  max_cardinality: 2
"#,
        )
        .unwrap();
        let schema = runtime_ir_to_knowledge(&ir);
        let dog = RuntimeObjectFact {
            id: "dog-1".to_owned(),
            object_type: "Dog".to_owned(),
            data: serde_json::json!({ "name": "Ada", "breed": "lab" }),
        };
        let edge = RuntimeRelationFact {
            source: "dog-1".to_owned(),
            relation: "companion_of".to_owned(),
            target: "dog-1".to_owned(),
        };
        // DL consistency is open-world: a missing edge does not make ABox
        // projection inconsistent, though it fails the executable closed-world
        // participation gate.
        assert!(
            runtime_world_to_knowledge(
                &schema,
                std::slice::from_ref(&dog),
                std::slice::from_ref(&edge)
            )
            .is_ok()
        );
        let isolated =
            runtime_world_to_knowledge(&schema, std::slice::from_ref(&dog), &[]).unwrap();
        assert!(Reasoner::new(&isolated).unwrap().is_consistent().unwrap());
        assert!(
            validate_canonical_ontology_with_world(&schema, std::slice::from_ref(&dog), &[])
                .is_err()
        );

        let other = RuntimeObjectFact {
            id: "dog-2".to_owned(),
            object_type: "Dog".to_owned(),
            data: serde_json::json!({ "name": "Ben", "breed": "poodle" }),
        };
        let missing =
            validate_canonical_ontology_with_world(&schema, &[dog, other], &[edge]).unwrap_err();
        assert!(missing.to_string().contains("individual `dog-2`"));
    }

    #[test]
    fn declared_scalar_cardinalities_project_and_gate_inherited_instances() {
        let ir = oaip_abi::parse_odl(
            r#"
object Animal:
  properties:
    name: string min_cardinality 1
object Dog extends Animal:
  properties:
    breed: string max_cardinality 0
"#,
        )
        .unwrap();
        assert!(validate_runtime_ir(&ir).is_ok());
        let schema = runtime_ir_to_knowledge(&ir);
        let source = class("Animal");
        assert!(schema.axioms().contains(&Axiom::subclass(
            source,
            ClassExpression::DataMinCardinality {
                cardinality: 1,
                property: Term::new("name"),
                datatype: Datatype::String,
            }
        )));

        let dog = RuntimeObjectFact {
            id: "dog-1".to_owned(),
            object_type: "Dog".to_owned(),
            data: serde_json::json!({ "name": "Ada" }),
        };
        // Open-world DL consistency does not turn an absent observation into a
        // contradiction; executable admission does.
        let missing_name = RuntimeObjectFact {
            id: "dog-2".to_owned(),
            object_type: "Dog".to_owned(),
            data: serde_json::json!({}),
        };
        let open_world =
            runtime_world_to_knowledge(&schema, std::slice::from_ref(&missing_name), &[]).unwrap();
        assert!(Reasoner::new(&open_world).unwrap().is_consistent().unwrap());
        assert!(validate_canonical_ontology_with_world(&schema, &[missing_name], &[]).is_err());

        let violations = declared_data_cardinality_violations(&schema, std::slice::from_ref(&dog));
        assert!(violations.is_empty());
        let forbidden_breed = RuntimeObjectFact {
            id: "dog-1".to_owned(),
            object_type: "Dog".to_owned(),
            data: serde_json::json!({ "name": "Ada", "breed": "lab" }),
        };
        let violations =
            declared_data_cardinality_violations(&schema, std::slice::from_ref(&forbidden_breed));
        assert_eq!(1, violations.len());
        assert_eq!("breed", violations[0].property);
        assert_eq!(Datatype::String, violations[0].datatype);
        assert_eq!(0, violations[0].max);
        assert_eq!(1, violations[0].actual);
    }

    #[test]
    fn declared_one_of_projects_into_the_canonical_tbox() {
        let ir = oaip_abi::parse_odl(
            r#"
object Customer:
  properties:
    status: string one_of [Active,Pending]
"#,
        )
        .unwrap();
        assert!(validate_runtime_ir(&ir).is_ok());
        let schema = runtime_ir_to_knowledge(&ir);
        assert!(schema.axioms().contains(&Axiom::DataPropertyOneOf {
            property: Term::new("status"),
            values: vec![
                Literal::String("Active".into()),
                Literal::String("Pending".into())
            ],
        }));

        let fact = RuntimeObjectFact {
            id: "customer-1".to_owned(),
            object_type: "Customer".to_owned(),
            data: serde_json::json!({ "status": "Draft" }),
        };
        let knowledge_base = runtime_world_to_knowledge(&schema, &[fact], &[]).unwrap();
        assert!(
            !Reasoner::new(&knowledge_base)
                .unwrap()
                .is_consistent()
                .unwrap()
        );
    }

    #[test]
    fn relation_characteristics_project_into_the_canonical_tbox() {
        fn relation(name: &str, inverse_of: Option<&str>) -> RelationDef {
            RelationDef {
                name: name.to_owned(),
                source: "Customer".to_owned(),
                target: "Customer".to_owned(),
                inverse_of: inverse_of.map(str::to_owned),
                ..RelationDef::default()
            }
        }

        let mut schema = ir(false);
        schema.relations = vec![
            RelationDef {
                functional: true,
                inverse_functional: true,
                symmetric: true,
                transitive: true,
                reflexive: true,
                irreflexive: false,
                asymmetric: false,
                ..relation("placed", Some("received"))
            },
            RelationDef {
                irreflexive: true,
                asymmetric: true,
                ..relation("received", Some("placed"))
            },
        ];
        let ontology = runtime_ir_to_knowledge(&schema);
        for axiom in [
            Axiom::FunctionalObjectProperty(Term::new("placed")),
            Axiom::InverseFunctionalObjectProperty(Term::new("placed")),
            Axiom::SymmetricObjectProperty(Term::new("placed")),
            Axiom::TransitiveObjectProperty(Term::new("placed")),
            Axiom::ReflexiveObjectProperty(Term::new("placed")),
            Axiom::IrreflexiveObjectProperty(Term::new("received")),
            Axiom::AsymmetricObjectProperty(Term::new("received")),
        ] {
            assert!(ontology.axioms().contains(&axiom), "missing {axiom:?}");
        }
        assert_eq!(
            1,
            ontology
                .axioms()
                .iter()
                .filter(|axiom| matches!(axiom, Axiom::InverseObjectProperties { .. }))
                .count()
        );
        assert!(validate_runtime_ir(&schema).is_ok());

        let mut contradictory = ir(false);
        contradictory.relations.push(RelationDef {
            symmetric: true,
            asymmetric: true,
            ..relation("bad", None)
        });
        assert!(validate_runtime_ir(&contradictory).is_err());

        let mut unknown_inverse = ir(false);
        unknown_inverse
            .relations
            .push(relation("orphan", Some("missing")));
        assert!(validate_runtime_ir(&unknown_inverse).is_err());

        let mut conflicting_inverse = ir(false);
        conflicting_inverse.relations = vec![
            relation("placed", Some("received")),
            relation("received", Some("other")),
        ];
        assert!(validate_runtime_ir(&conflicting_inverse).is_err());

        let mut one_sided_inverse = ir(false);
        one_sided_inverse.relations = vec![
            relation("placed", Some("received")),
            relation("received", None),
        ];
        assert!(validate_runtime_ir(&one_sided_inverse).is_ok());
    }

    #[test]
    fn replacement_removes_old_projection_and_adds_new_projection() {
        let old = ir(false);
        let mut new = ir(false);
        new.object_types[0].properties.push(PropertyDef {
            name: "score".to_owned(),
            ty: ScalarType::Int,
            constraints: Vec::new(),
        });
        let mut base = runtime_ir_to_knowledge(&old);
        base.push(Axiom::subclass(class("Dog"), class("Animal")));
        let canonical = canonical_runtime_ontology(&base, Some(&old), Some(&new));
        assert_eq!(8, canonical.len());
        assert!(matches!(canonical.axioms()[0], Axiom::SubClassOf { .. }));
    }
}
