//! OWL Functional Syntax (OFN) import/export for external tools.

use crate::error::{KnowledgeError, Result};
use crate::model::{
    Axiom, ClassExpression, DataFacet, DataRange, Datatype, FacetValue, Literal, Ontology, Term,
};
use crate::reasoner::{display_term, to_horned_owl};
use chrono::{NaiveDate, SecondsFormat, Utc};
use horned_owl::io::ofn;
use horned_owl::model::{ArcStr, ObjectProperty};
use horned_owl::ontology::component_mapped::ArcComponentMappedOntology;
use horned_owl::ontology::set::SetOntology;
use horned_owl::vocab::Facet;
use std::io::Cursor;

type OwlComponent = horned_owl::model::Component<ArcStr>;
type OwlClassExpression = horned_owl::model::ClassExpression<ArcStr>;
type OwlDataRange = horned_owl::model::DataRange<ArcStr>;
type OwlLiteral = horned_owl::model::Literal<ArcStr>;
type OwlObjectPropertyExpression = horned_owl::model::ObjectPropertyExpression<ArcStr>;

/// Serialize the ontology to OWL 2 Functional Syntax.
pub fn to_owl_ofn(ontology: &Ontology) -> Result<String> {
    let set = to_horned_owl(ontology)?;
    let mapped = ArcComponentMappedOntology::from(set);
    let mut buffer = Vec::new();
    ofn::writer::write(&mut buffer, &mapped, None).map_err(|error| {
        KnowledgeError::Invalid(format!(
            "OWL Functional Syntax serialization failed: {error}"
        ))
    })?;
    String::from_utf8(buffer)
        .map_err(|error| KnowledgeError::Invalid(format!("serialized OFN is not UTF-8: {error}")))
}

/// Parse OWL 2 Functional Syntax into the kernel's axiom model.
///
/// Declarations and ontology metadata are intentionally not represented in the
/// kernel; every unsupported logical axiom is rejected instead of being dropped.
pub fn from_owl_ofn(source: &str) -> Result<Ontology> {
    let cursor = Cursor::new(source);
    let (mapped, _prefixes) =
        ofn::reader::read::<ArcStr, ArcComponentMappedOntology, _>(cursor, Default::default())
            .map_err(|error| {
                KnowledgeError::Invalid(format!("OWL Functional Syntax parse failed: {error}"))
            })?;
    convert_ontology(SetOntology::from(mapped))
}

fn invalid(reason: impl std::fmt::Display) -> KnowledgeError {
    KnowledgeError::Invalid(format!("unsupported OWL Functional Syntax: {reason}"))
}

fn term(iri: impl AsRef<str>) -> Term {
    display_term(iri)
}

fn direct_property(property: &OwlObjectPropertyExpression) -> Result<ObjectProperty<ArcStr>> {
    match property {
        OwlObjectPropertyExpression::ObjectProperty(property) => Ok(property.clone()),
        OwlObjectPropertyExpression::InverseObjectProperty(_) => Err(invalid(
            "inverse property expressions are outside the kernel class language",
        )),
    }
}

fn named_individual(individual: &horned_owl::model::Individual<ArcStr>) -> Result<Term> {
    match individual {
        horned_owl::model::Individual::Named(individual) => Ok(term(individual.0.as_ref())),
        horned_owl::model::Individual::Anonymous(_) => {
            Err(invalid("anonymous individuals are not supported"))
        }
    }
}

fn convert_datatype(iri: impl AsRef<str>) -> Result<Datatype> {
    match iri.as_ref() {
        "http://www.w3.org/2001/XMLSchema#string" => Ok(Datatype::String),
        "http://www.w3.org/2001/XMLSchema#integer" => Ok(Datatype::Integer),
        "http://www.w3.org/2001/XMLSchema#double" => Ok(Datatype::Double),
        "http://www.w3.org/2001/XMLSchema#boolean" => Ok(Datatype::Boolean),
        "http://www.w3.org/2001/XMLSchema#date" => Ok(Datatype::Date),
        "http://www.w3.org/2001/XMLSchema#dateTime" => Ok(Datatype::DateTime),
        "http://www.w3.org/2001/XMLSchema#decimal" => Ok(Datatype::Decimal),
        iri => Err(invalid(format!("datatype `{iri}` is not supported"))),
    }
}

fn literal(value: &OwlLiteral) -> Result<Literal> {
    match value {
        OwlLiteral::Simple { literal } => Ok(Literal::String(literal.clone())),
        OwlLiteral::Language { .. } => Err(invalid("language-tagged literals are not supported")),
        OwlLiteral::Datatype {
            literal,
            datatype_iri,
        } => typed_literal(datatype_iri.as_ref(), literal),
    }
}

fn typed_literal(datatype: &str, value: &str) -> Result<Literal> {
    let invalid_value = || invalid(format!("`{value}` is not a valid `{datatype}` literal"));
    match datatype {
        "http://www.w3.org/2001/XMLSchema#string" => Ok(Literal::String(value.to_owned())),
        "http://www.w3.org/2001/XMLSchema#boolean" => value
            .parse()
            .map(Literal::Boolean)
            .map_err(|_| invalid_value()),
        "http://www.w3.org/2001/XMLSchema#integer" => value
            .parse()
            .map(Literal::Integer)
            .map_err(|_| invalid_value()),
        "http://www.w3.org/2001/XMLSchema#double" => parse_double(value).ok_or_else(invalid_value),
        "http://www.w3.org/2001/XMLSchema#decimal" => oaip_abi::canonical_decimal(value)
            .map(Literal::Decimal)
            .ok_or_else(invalid_value),
        "http://www.w3.org/2001/XMLSchema#date" => NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .ok()
            .filter(|parsed| parsed.to_string() == value)
            .map(|parsed| Literal::Date(parsed.to_string()))
            .ok_or_else(invalid_value),
        "http://www.w3.org/2001/XMLSchema#dateTime" => chrono::DateTime::parse_from_rfc3339(value)
            .map(|parsed| {
                Literal::DateTime(
                    parsed
                        .with_timezone(&Utc)
                        .to_rfc3339_opts(SecondsFormat::AutoSi, true),
                )
            })
            .map_err(|_| invalid_value()),
        _ => Err(invalid(format!("datatype `{datatype}` is not supported"))),
    }
}

fn parse_double(value: &str) -> Option<Literal> {
    let parsed = match value {
        "INF" => f64::INFINITY,
        "-INF" => f64::NEG_INFINITY,
        "NaN" => f64::NAN,
        value => value.parse().ok()?,
    };
    Some(Literal::Double(parsed))
}

fn facet(facet: &Facet) -> Option<DataFacet> {
    match facet {
        Facet::MinInclusive => Some(DataFacet::MinInclusive),
        Facet::MinExclusive => Some(DataFacet::MinExclusive),
        Facet::MaxInclusive => Some(DataFacet::MaxInclusive),
        Facet::MaxExclusive => Some(DataFacet::MaxExclusive),
        Facet::MinLength => Some(DataFacet::MinLength),
        Facet::MaxLength => Some(DataFacet::MaxLength),
        Facet::TotalDigits => Some(DataFacet::TotalDigits),
        Facet::FractionDigits => Some(DataFacet::FractionDigits),
        Facet::Pattern => Some(DataFacet::Pattern),
        _ => None,
    }
}

fn data_range(range: &OwlDataRange) -> Result<DataRange> {
    match range {
        OwlDataRange::Datatype(datatype) => {
            Ok(DataRange::Datatype(convert_datatype(datatype.0.as_ref())?))
        }
        OwlDataRange::DataOneOf(values) => Ok(DataRange::OneOf(
            values.iter().map(literal).collect::<Result<_>>()?,
        )),
        OwlDataRange::DatatypeRestriction(base, restrictions) => {
            let facets = restrictions
                .iter()
                .map(|restriction| {
                    facet(&restriction.f)
                        .zip(literal(&restriction.l).ok())
                        .map(|(facet, value)| FacetValue { facet, value })
                        .ok_or_else(|| invalid("unsupported datatype restriction facet"))
                })
                .collect::<Result<_>>()?;
            Ok(DataRange::Restriction {
                datatype: convert_datatype(base.0.as_ref())?,
                facets,
            })
        }
        OwlDataRange::DataComplementOf(child) => {
            Ok(DataRange::Complement(Box::new(data_range(child)?)))
        }
        OwlDataRange::DataUnionOf(children) => Ok(DataRange::Union(
            children.iter().map(data_range).collect::<Result<_>>()?,
        )),
        OwlDataRange::DataIntersectionOf(children) => Ok(DataRange::Intersection(
            children.iter().map(data_range).collect::<Result<_>>()?,
        )),
    }
}

fn simple_data_range(range: &OwlDataRange) -> Result<Datatype> {
    match range {
        OwlDataRange::Datatype(datatype) => convert_datatype(datatype.0.as_ref()),
        _ => Err(invalid(
            "complex data ranges in class cardinalities are not supported",
        )),
    }
}

fn object_property(property: &OwlObjectPropertyExpression) -> Result<Term> {
    direct_property(property).map(|property| term(property.0.as_ref()))
}

fn data_property(property: &horned_owl::model::DataProperty<ArcStr>) -> Term {
    term(property.0.as_ref())
}

fn class_expression(expression: &OwlClassExpression) -> Result<ClassExpression> {
    const THING: &str = "http://www.w3.org/2002/07/owl#Thing";
    const NOTHING: &str = "http://www.w3.org/2002/07/owl#Nothing";

    match expression {
        OwlClassExpression::Class(class) if class.0.as_ref() == THING => Ok(ClassExpression::Thing),
        OwlClassExpression::Class(class) if class.0.as_ref() == NOTHING => {
            Ok(ClassExpression::Nothing)
        }
        OwlClassExpression::Class(class) => Ok(ClassExpression::Class(term(class.0.as_ref()))),
        OwlClassExpression::ObjectIntersectionOf(expressions) => Ok(ClassExpression::Intersection(
            expressions
                .iter()
                .map(class_expression)
                .collect::<Result<_>>()?,
        )),
        OwlClassExpression::ObjectUnionOf(expressions) => Ok(ClassExpression::Union(
            expressions
                .iter()
                .map(class_expression)
                .collect::<Result<_>>()?,
        )),
        OwlClassExpression::ObjectComplementOf(expression) => Ok(ClassExpression::Complement(
            Box::new(class_expression(expression)?),
        )),
        OwlClassExpression::ObjectOneOf(individuals) => Ok(ClassExpression::ObjectOneOf(
            individuals
                .iter()
                .map(named_individual)
                .collect::<Result<_>>()?,
        )),
        OwlClassExpression::ObjectSomeValuesFrom { ope, bce } => {
            Ok(ClassExpression::ObjectSomeValuesFrom {
                property: object_property(ope)?,
                value: Box::new(class_expression(bce)?),
            })
        }
        OwlClassExpression::ObjectAllValuesFrom { ope, bce } => {
            Ok(ClassExpression::ObjectAllValuesFrom {
                property: object_property(ope)?,
                value: Box::new(class_expression(bce)?),
            })
        }
        OwlClassExpression::ObjectHasValue { ope, i } => Ok(ClassExpression::ObjectHasValue {
            property: object_property(ope)?,
            individual: named_individual(i)?,
        }),
        OwlClassExpression::ObjectMinCardinality { n, ope, bce } => {
            Ok(ClassExpression::ObjectMinCardinality {
                cardinality: *n,
                property: object_property(ope)?,
                value: Box::new(class_expression(bce)?),
            })
        }
        OwlClassExpression::ObjectMaxCardinality { n, ope, bce } => {
            Ok(ClassExpression::ObjectMaxCardinality {
                cardinality: *n,
                property: object_property(ope)?,
                value: Box::new(class_expression(bce)?),
            })
        }
        OwlClassExpression::DataSomeValuesFrom { dp, dr } => {
            Ok(ClassExpression::DataSomeValuesFrom {
                property: data_property(dp),
                datatype: simple_data_range(dr)?,
            })
        }
        OwlClassExpression::DataAllValuesFrom { dp, dr } => {
            Ok(ClassExpression::DataAllValuesFrom {
                property: data_property(dp),
                datatype: simple_data_range(dr)?,
            })
        }
        OwlClassExpression::DataHasValue { dp, l } => Ok(ClassExpression::DataHasValue {
            property: data_property(dp),
            value: literal(l)?,
        }),
        OwlClassExpression::DataMinCardinality { n, dp, dr } => {
            Ok(ClassExpression::DataMinCardinality {
                cardinality: *n,
                property: data_property(dp),
                datatype: simple_data_range(dr)?,
            })
        }
        OwlClassExpression::DataMaxCardinality { n, dp, dr } => {
            Ok(ClassExpression::DataMaxCardinality {
                cardinality: *n,
                property: data_property(dp),
                datatype: simple_data_range(dr)?,
            })
        }
        OwlClassExpression::ObjectHasSelf(_)
        | OwlClassExpression::ObjectExactCardinality { .. }
        | OwlClassExpression::DataExactCardinality { .. } => Err(invalid(
            "self restrictions and exact cardinalities are outside the kernel class language",
        )),
    }
}

fn property_range_axiom(
    property: &horned_owl::model::DataProperty<ArcStr>,
    range: &OwlDataRange,
) -> Result<Axiom> {
    let property = data_property(property);
    match range {
        OwlDataRange::Datatype(datatype) => Ok(Axiom::DataPropertyRange {
            property,
            datatype: convert_datatype(datatype.0.as_ref())?,
        }),
        OwlDataRange::DataOneOf(values) => Ok(Axiom::DataPropertyOneOf {
            property,
            values: values.iter().map(literal).collect::<Result<_>>()?,
        }),
        OwlDataRange::DatatypeRestriction(base, restrictions) => {
            let range = data_range(&OwlDataRange::DatatypeRestriction(
                base.clone(),
                restrictions.clone(),
            ))?;
            let DataRange::Restriction { datatype, facets } = range else {
                unreachable!("the constructed range is always a restriction");
            };
            Ok(Axiom::DataPropertyRangeRestriction {
                property,
                datatype,
                facets,
            })
        }
        range => Ok(Axiom::DataPropertyDataRange {
            property,
            range: data_range(range)?,
        }),
    }
}

fn axiom(component: &OwlComponent) -> Result<Option<Axiom>> {
    use horned_owl::model::*;

    let axiom = match component {
        Component::SubClassOf(axiom) => Axiom::SubClassOf {
            sub: class_expression(&axiom.sub)?,
            sup: class_expression(&axiom.sup)?,
        },
        Component::EquivalentClasses(axiom) => Axiom::EquivalentClasses(
            axiom
                .0
                .iter()
                .map(class_expression)
                .collect::<Result<_>>()?,
        ),
        Component::DisjointClasses(axiom) => Axiom::DisjointClasses(
            axiom
                .0
                .iter()
                .map(class_expression)
                .collect::<Result<_>>()?,
        ),
        Component::ObjectPropertyDomain(axiom) => Axiom::ObjectPropertyDomain {
            property: object_property(&axiom.ope)?,
            class: class_expression(&axiom.ce)?,
        },
        Component::ObjectPropertyRange(axiom) => Axiom::ObjectPropertyRange {
            property: object_property(&axiom.ope)?,
            class: class_expression(&axiom.ce)?,
        },
        Component::SubObjectPropertyOf(axiom) => match &axiom.sub {
            SubObjectPropertyExpression::ObjectPropertyExpression(sub) => {
                Axiom::SubObjectPropertyOf {
                    sub: object_property(sub)?,
                    sup: object_property(&axiom.sup)?,
                }
            }
            SubObjectPropertyExpression::ObjectPropertyChain(properties) => {
                Axiom::SubObjectPropertyChainOf {
                    properties: properties
                        .iter()
                        .map(object_property)
                        .collect::<Result<_>>()?,
                    super_property: object_property(&axiom.sup)?,
                }
            }
        },
        Component::DisjointObjectProperties(properties) => Axiom::DisjointObjectProperties(
            properties
                .0
                .iter()
                .map(object_property)
                .collect::<Result<_>>()?,
        ),
        Component::TransitiveObjectProperty(property) => {
            Axiom::TransitiveObjectProperty(object_property(&property.0)?)
        }
        Component::SymmetricObjectProperty(property) => {
            Axiom::SymmetricObjectProperty(object_property(&property.0)?)
        }
        Component::FunctionalObjectProperty(property) => {
            Axiom::FunctionalObjectProperty(object_property(&property.0)?)
        }
        Component::InverseFunctionalObjectProperty(property) => {
            Axiom::InverseFunctionalObjectProperty(object_property(&property.0)?)
        }
        Component::ReflexiveObjectProperty(property) => {
            Axiom::ReflexiveObjectProperty(object_property(&property.0)?)
        }
        Component::IrreflexiveObjectProperty(property) => {
            Axiom::IrreflexiveObjectProperty(object_property(&property.0)?)
        }
        Component::AsymmetricObjectProperty(property) => {
            Axiom::AsymmetricObjectProperty(object_property(&property.0)?)
        }
        Component::InverseObjectProperties(pair) => Axiom::InverseObjectProperties {
            first: object_property(&pair.0.clone().into())?,
            second: object_property(&pair.1.clone().into())?,
        },
        Component::SubDataPropertyOf(axiom) => Axiom::SubDataPropertyOf {
            sub: data_property(&axiom.sub),
            sup: data_property(&axiom.sup),
        },
        Component::DisjointDataProperties(properties) => {
            Axiom::DisjointDataProperties(properties.0.iter().map(data_property).collect())
        }
        Component::DataPropertyDomain(axiom) => Axiom::DataPropertyDomain {
            property: data_property(&axiom.dp),
            class: class_expression(&axiom.ce)?,
        },
        Component::DataPropertyRange(axiom) => property_range_axiom(&axiom.dp, &axiom.dr)?,
        Component::FunctionalDataProperty(property) => {
            Axiom::FunctionalDataProperty(data_property(&property.0))
        }
        Component::ClassAssertion(axiom) => Axiom::ClassAssertion {
            class: class_expression(&axiom.ce)?,
            individual: named_individual(&axiom.i)?,
        },
        Component::ObjectPropertyAssertion(axiom) => Axiom::ObjectPropertyAssertion {
            property: object_property(&axiom.ope)?,
            subject: named_individual(&axiom.from)?,
            object: named_individual(&axiom.to)?,
        },
        Component::NegativeObjectPropertyAssertion(axiom) => {
            Axiom::NegativeObjectPropertyAssertion {
                property: object_property(&axiom.ope)?,
                subject: named_individual(&axiom.from)?,
                object: named_individual(&axiom.to)?,
            }
        }
        Component::DataPropertyAssertion(axiom) => Axiom::DataPropertyAssertion {
            property: data_property(&axiom.dp),
            individual: named_individual(&axiom.from)?,
            value: literal(&axiom.to)?,
        },
        Component::NegativeDataPropertyAssertion(axiom) => Axiom::NegativeDataPropertyAssertion {
            property: data_property(&axiom.dp),
            individual: named_individual(&axiom.from)?,
            value: literal(&axiom.to)?,
        },
        Component::SameIndividual(axiom) => Axiom::SameIndividuals(
            axiom
                .0
                .iter()
                .map(named_individual)
                .collect::<Result<_>>()?,
        ),
        Component::DifferentIndividuals(axiom) => Axiom::DifferentIndividuals(
            axiom
                .0
                .iter()
                .map(named_individual)
                .collect::<Result<_>>()?,
        ),
        Component::DeclareClass(_)
        | Component::DeclareObjectProperty(_)
        | Component::DeclareAnnotationProperty(_)
        | Component::DeclareDataProperty(_)
        | Component::DeclareNamedIndividual(_)
        | Component::DeclareDatatype(_)
        | Component::OntologyID(_)
        | Component::DocIRI(_)
        | Component::Import(_)
        | Component::OntologyAnnotation(_)
        | Component::AnnotationAssertion(_) => return Ok(None),
        component => {
            return Err(invalid(format!(
                "unmapped logical component `{:?}`",
                component.kind()
            )));
        }
    };
    Ok(Some(axiom))
}

fn convert_ontology(set: SetOntology<ArcStr>) -> Result<Ontology> {
    let mut ontology = Ontology::new();
    for annotated in &set {
        if let Some(axiom) = axiom(&annotated.component)? {
            ontology.push(axiom);
        }
    }
    Ok(ontology)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reasoner::Reasoner;

    fn sample() -> Ontology {
        Ontology::new()
            .with_axiom(Axiom::SubClassOf {
                sub: ClassExpression::class("http://example.com/owl#Researcher"),
                sup: ClassExpression::some_values_from(
                    "http://example.com/owl#worksOn",
                    ClassExpression::class("http://example.com/owl#Project"),
                ),
            })
            .with_axiom(Axiom::SubObjectPropertyChainOf {
                properties: vec![
                    Term::new("http://example.com/owl#leads"),
                    Term::new("http://example.com/owl#hasMember"),
                ],
                super_property: Term::new("http://example.com/owl#supervises"),
            })
            .with_axiom(Axiom::DisjointObjectProperties(vec![
                Term::new("http://example.com/owl#internal"),
                Term::new("http://example.com/owl#external"),
            ]))
            .with_axiom(Axiom::DataPropertyRangeRestriction {
                property: Term::new("http://example.com/owl#age"),
                datatype: Datatype::Integer,
                facets: vec![FacetValue {
                    facet: DataFacet::MinInclusive,
                    value: Literal::Integer(18),
                }],
            })
            .with_axiom(Axiom::ClassAssertion {
                class: ClassExpression::class("http://example.com/owl#Researcher"),
                individual: Term::new("http://example.com/owl#alice"),
            })
            .with_axiom(Axiom::DataPropertyAssertion {
                property: Term::new("http://example.com/owl#age"),
                individual: Term::new("http://example.com/owl#alice"),
                value: Literal::Integer(30),
            })
            .with_axiom(Axiom::SameIndividuals(vec![
                Term::new("http://example.com/owl#alice"),
                Term::new("http://example.com/owl#researcher-1"),
            ]))
    }

    #[test]
    fn owl_functional_syntax_round_trips_kernel_ontology() {
        let ontology = sample();
        let syntax = to_owl_ofn(&ontology).unwrap();
        let parsed = from_owl_ofn(&syntax).unwrap();
        assert_eq!(to_owl_ofn(&parsed).unwrap(), syntax);
    }

    #[test]
    fn imported_ontology_reasons_like_the_source() {
        let mut ontology = sample();
        ontology.push(Axiom::ObjectPropertyAssertion {
            property: Term::new("http://example.com/owl#leads"),
            subject: Term::new("http://example.com/owl#alice"),
            object: Term::new("http://example.com/owl#project-1"),
        });
        ontology.push(Axiom::ObjectPropertyAssertion {
            property: Term::new("http://example.com/owl#hasMember"),
            subject: Term::new("http://example.com/owl#project-1"),
            object: Term::new("http://example.com/owl#member-1"),
        });
        let syntax = to_owl_ofn(&ontology).unwrap();
        let parsed = from_owl_ofn(&syntax).unwrap();
        assert_eq!(to_owl_ofn(&parsed).unwrap(), syntax);

        let reasoner = Reasoner::new(&parsed).unwrap();
        assert!(reasoner.is_consistent().unwrap());
        assert!(
            reasoner
                .entails_object_property(
                    &Term::new("http://example.com/owl#supervises"),
                    &Term::new("http://example.com/owl#alice"),
                    &Term::new("http://example.com/owl#member-1"),
                )
                .unwrap()
        );
    }

    #[test]
    fn unsupported_logical_syntax_is_rejected_not_dropped() {
        let source = r#"Prefix(:=<http://example.com/owl#>)
Ontology(
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(ObjectProperty(:p))
    SubClassOf(:A ObjectExactCardinality(1 :p :B))
)"#;
        let result = from_owl_ofn(source);
        assert!(matches!(
            result,
            Err(KnowledgeError::Invalid(message)) if message.contains("outside the kernel")
        ));
    }
}
