//! Knowledge representation vocabulary shared by parsers, stores and reasoners.
//!
//! The model intentionally keeps syntax and derived conclusions separate.  An
//! [`Ontology`] is an append-only list of asserted axioms; every conclusion is
//! computed into a separate justification-carrying result.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Stable identifier for a named class, property or individual.
///
/// Names are opaque to the reasoner.  Parsers normally store IRIs here, while
/// embedded callers may use compact local names without losing semantics.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Term(pub String);

impl Term {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for Term {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}
impl From<String> for Term {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl std::fmt::Display for Term {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// OWL 2 typed literals used by data-property assertions and restrictions.
///
/// Runtime JSON numbers are preserved as an integer or IEEE double; strings use
/// RDF simple-literal semantics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Literal {
    Boolean(bool),
    Integer(i64),
    Double(f64),
    String(String),
    /// XSD decimal stored as an exact canonical lexical value.
    Decimal(String),
    /// XSD date stored as a canonical calendar date without a timezone.
    Date(String),
    /// XSD dateTime stored in canonical UTC form after boundary validation.
    DateTime(String),
}

impl Literal {
    /// The XSI datatype inferred from the Rust/runtime representation.
    pub fn datatype(&self) -> Datatype {
        match self {
            Self::Boolean(_) => Datatype::Boolean,
            Self::Integer(_) => Datatype::Integer,
            Self::Double(_) => Datatype::Double,
            Self::String(_) => Datatype::String,
            Self::Decimal(_) => Datatype::Decimal,
            Self::Date(_) => Datatype::Date,
            Self::DateTime(_) => Datatype::DateTime,
        }
    }

    pub fn lexical_form(self) -> String {
        match self {
            Self::Boolean(value) => value.to_string(),
            Self::Integer(value) => value.to_string(),
            Self::Double(value) => format!("{value:?}"),
            Self::String(value) => value,
            Self::Decimal(value) => value.clone(),
            Self::Date(value) => value.clone(),
            Self::DateTime(value) => value.clone(),
        }
    }
}

/// XSD datatypes supported by the first complete bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Datatype {
    String,
    Integer,
    Double,
    Boolean,
    Date,
    DateTime,
    Decimal,
}

impl Datatype {
    pub fn iri(self) -> &'static str {
        match self {
            Self::String => "http://www.w3.org/2001/XMLSchema#string",
            Self::Integer => "http://www.w3.org/2001/XMLSchema#integer",
            Self::Double => "http://www.w3.org/2001/XMLSchema#double",
            Self::Boolean => "http://www.w3.org/2001/XMLSchema#boolean",
            Self::Date => "http://www.w3.org/2001/XMLSchema#date",
            Self::DateTime => "http://www.w3.org/2001/XMLSchema#dateTime",
            Self::Decimal => "http://www.w3.org/2001/XMLSchema#decimal",
        }
    }
}

/// Boolean/value restriction language used by the first reasoning kernel.
///
/// This is richer than the object-only ODL IR: it can state necessary and
/// sufficient conditions over relationships as well as typed data values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClassExpression {
    /// The full domain (OWL Thing / ⊤).
    Thing,
    /// The empty concept (OWL Nothing / ⊥).
    Nothing,
    Class(Term),
    Intersection(Vec<ClassExpression>),
    Union(Vec<ClassExpression>),
    Complement(Box<ClassExpression>),
    ObjectSomeValuesFrom {
        property: Term,
        value: Box<ClassExpression>,
    },
    ObjectAllValuesFrom {
        property: Term,
        value: Box<ClassExpression>,
    },
    ObjectHasValue {
        property: Term,
        individual: Term,
    },
    ObjectMinCardinality {
        cardinality: u32,
        property: Term,
        value: Box<ClassExpression>,
    },
    ObjectMaxCardinality {
        cardinality: u32,
        property: Term,
        value: Box<ClassExpression>,
    },
    DataSomeValuesFrom {
        property: Term,
        datatype: Datatype,
    },
    DataAllValuesFrom {
        property: Term,
        datatype: Datatype,
    },
    DataHasValue {
        property: Term,
        value: Literal,
    },
    DataMinCardinality {
        cardinality: u32,
        property: Term,
        datatype: Datatype,
    },
    DataMaxCardinality {
        cardinality: u32,
        property: Term,
        datatype: Datatype,
    },
}

impl ClassExpression {
    pub fn class(name: impl Into<Term>) -> Self {
        Self::Class(name.into())
    }

    pub fn intersection(expressions: impl IntoIterator<Item = ClassExpression>) -> Self {
        Self::Intersection(expressions.into_iter().collect())
    }

    pub fn union(expressions: impl IntoIterator<Item = ClassExpression>) -> Self {
        Self::Union(expressions.into_iter().collect())
    }

    pub fn complement(expression: ClassExpression) -> Self {
        Self::Complement(Box::new(expression))
    }

    pub fn some_values_from(property: impl Into<Term>, value: ClassExpression) -> Self {
        Self::ObjectSomeValuesFrom {
            property: property.into(),
            value: Box::new(value),
        }
    }

    pub fn all_values_from(property: impl Into<Term>, value: ClassExpression) -> Self {
        Self::ObjectAllValuesFrom {
            property: property.into(),
            value: Box::new(value),
        }
    }

    pub fn has_value(property: impl Into<Term>, individual: impl Into<Term>) -> Self {
        Self::ObjectHasValue {
            property: property.into(),
            individual: individual.into(),
        }
    }

    pub fn min_cardinality(
        cardinality: u32,
        property: impl Into<Term>,
        value: ClassExpression,
    ) -> Self {
        Self::ObjectMinCardinality {
            cardinality,
            property: property.into(),
            value: Box::new(value),
        }
    }

    pub fn some_data(property: impl Into<Term>, datatype: Datatype) -> Self {
        Self::DataSomeValuesFrom {
            property: property.into(),
            datatype,
        }
    }

    pub fn all_data(property: impl Into<Term>, datatype: Datatype) -> Self {
        Self::DataAllValuesFrom {
            property: property.into(),
            datatype,
        }
    }

    pub fn has_data(property: impl Into<Term>, value: Literal) -> Self {
        Self::DataHasValue {
            property: property.into(),
            value,
        }
    }

    pub fn max_cardinality(
        cardinality: u32,
        property: impl Into<Term>,
        value: ClassExpression,
    ) -> Self {
        Self::ObjectMaxCardinality {
            cardinality,
            property: property.into(),
            value: Box::new(value),
        }
    }

    /// All named classes syntactically mentioned by this expression.
    ///
    /// The reasoner uses this as a safe conservative seed.  It deliberately does
    /// **not** infer references hidden behind negation as named memberships.
    pub fn named_classes(&self) -> BTreeSet<Term> {
        let mut out = BTreeSet::new();
        self.push_named_classes(&mut out);
        out
    }

    fn push_named_classes(&self, out: &mut BTreeSet<Term>) {
        match self {
            Self::Thing | Self::Nothing => {}
            Self::Class(term) => {
                out.insert(term.clone());
            }
            Self::Intersection(items) | Self::Union(items) => {
                for item in items {
                    item.push_named_classes(out);
                }
            }
            Self::Complement(item)
            | Self::ObjectSomeValuesFrom { value: item, .. }
            | Self::ObjectAllValuesFrom { value: item, .. }
            | Self::ObjectMinCardinality { value: item, .. }
            | Self::ObjectMaxCardinality { value: item, .. } => item.push_named_classes(out),
            Self::ObjectHasValue { .. }
            | Self::DataSomeValuesFrom { .. }
            | Self::DataAllValuesFrom { .. }
            | Self::DataHasValue { .. }
            | Self::DataMinCardinality { .. }
            | Self::DataMaxCardinality { .. } => {}
        }
    }
}

/// A single XSD constraining facet supported by the knowledge kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DataFacet {
    MinInclusive,
    MinExclusive,
    MaxInclusive,
    MaxExclusive,
    MinLength,
    MaxLength,
    TotalDigits,
    FractionDigits,
    Pattern,
}

impl DataFacet {
    pub fn iri(self) -> &'static str {
        match self {
            Self::MinInclusive => "http://www.w3.org/2001/XMLSchema#minInclusive",
            Self::MinExclusive => "http://www.w3.org/2001/XMLSchema#minExclusive",
            Self::MaxInclusive => "http://www.w3.org/2001/XMLSchema#maxInclusive",
            Self::MaxExclusive => "http://www.w3.org/2001/XMLSchema#maxExclusive",
            Self::MinLength => "http://www.w3.org/2001/XMLSchema#minLength",
            Self::MaxLength => "http://www.w3.org/2001/XMLSchema#maxLength",
            Self::TotalDigits => "http://www.w3.org/2001/XMLSchema#totalDigits",
            Self::FractionDigits => "http://www.w3.org/2001/XMLSchema#fractionDigits",
            Self::Pattern => "http://www.w3.org/2001/XMLSchema#pattern",
        }
    }
}

/// One `FacetRestriction` inside an OWL datatype restriction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FacetValue {
    pub facet: DataFacet,
    pub value: Literal,
}

/// A composable OWL 2 DL data-range expression.
///
/// The existing flat property-range axioms remain the canonical executable
/// projection.  This expression adds governed proposals the ability to state
/// explicit unions and intersections of datatypes, enumerations and facets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DataRange {
    Datatype(Datatype),
    OneOf(Vec<Literal>),
    Restriction {
        datatype: Datatype,
        facets: Vec<FacetValue>,
    },
    Complement(Box<DataRange>),
    Union(Vec<DataRange>),
    Intersection(Vec<DataRange>),
}

impl DataRange {
    pub fn datatype(datatype: Datatype) -> Self {
        Self::Datatype(datatype)
    }

    pub fn one_of(values: impl IntoIterator<Item = Literal>) -> Self {
        Self::OneOf(values.into_iter().collect())
    }

    pub fn restriction(datatype: Datatype, facets: Vec<FacetValue>) -> Self {
        Self::Restriction { datatype, facets }
    }

    pub fn complement(range: DataRange) -> Self {
        Self::Complement(Box::new(range))
    }

    pub fn union(ranges: impl IntoIterator<Item = DataRange>) -> Self {
        Self::Union(ranges.into_iter().collect())
    }

    pub fn intersection(ranges: impl IntoIterator<Item = DataRange>) -> Self {
        Self::Intersection(ranges.into_iter().collect())
    }
}

/// Asserted TBox/ABox axioms supported by the knowledge kernel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Axiom {
    SubClassOf {
        sub: ClassExpression,
        sup: ClassExpression,
    },
    EquivalentClasses(Vec<ClassExpression>),
    DisjointClasses(Vec<ClassExpression>),
    ObjectPropertyDomain {
        property: Term,
        class: ClassExpression,
    },
    ObjectPropertyRange {
        property: Term,
        class: ClassExpression,
    },
    SubObjectPropertyOf {
        sub: Term,
        sup: Term,
    },
    TransitiveObjectProperty(Term),
    SymmetricObjectProperty(Term),
    FunctionalObjectProperty(Term),
    InverseFunctionalObjectProperty(Term),
    ReflexiveObjectProperty(Term),
    IrreflexiveObjectProperty(Term),
    AsymmetricObjectProperty(Term),
    InverseObjectProperties {
        first: Term,
        second: Term,
    },
    SubDataPropertyOf {
        sub: Term,
        sup: Term,
    },
    DisjointDataProperties(Vec<Term>),
    DataPropertyDomain {
        property: Term,
        class: ClassExpression,
    },
    DataPropertyRange {
        property: Term,
        datatype: Datatype,
    },
    /// OWL `DataOneOf` used as a property range.
    ///
    /// Multiple enumeration ranges intersect: a value is admissible only when
    /// every enumerated range admits it.
    DataPropertyOneOf {
        property: Term,
        values: Vec<Literal>,
    },
    /// OWL datatype restriction used as a property range.
    ///
    /// Multiple restricted ranges intersect.  The base datatype is explicit so
    /// every restriction has a canonical value space even before assertions.
    DataPropertyRangeRestriction {
        property: Term,
        datatype: Datatype,
        facets: Vec<FacetValue>,
    },
    /// General composable data-property range used by governed proposals.
    DataPropertyDataRange {
        property: Term,
        range: DataRange,
    },
    FunctionalDataProperty(Term),
    ClassAssertion {
        class: ClassExpression,
        individual: Term,
    },
    ObjectPropertyAssertion {
        property: Term,
        subject: Term,
        object: Term,
    },
    NegativeObjectPropertyAssertion {
        property: Term,
        subject: Term,
        object: Term,
    },
    DataPropertyAssertion {
        property: Term,
        individual: Term,
        value: Literal,
    },
    NegativeDataPropertyAssertion {
        property: Term,
        individual: Term,
        value: Literal,
    },
    SameIndividuals(Vec<Term>),
    DifferentIndividuals(Vec<Term>),
}

impl Axiom {
    pub fn subclass(sub: ClassExpression, sup: ClassExpression) -> Self {
        Self::SubClassOf { sub, sup }
    }

    pub fn equivalent(expressions: impl IntoIterator<Item = ClassExpression>) -> Self {
        Self::EquivalentClasses(expressions.into_iter().collect())
    }

    pub fn disjoint(expressions: impl IntoIterator<Item = ClassExpression>) -> Self {
        Self::DisjointClasses(expressions.into_iter().collect())
    }
}

/// An append-only set of asserted axioms.  Derived facts never mutate it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Ontology {
    axioms: Vec<Axiom>,
}

impl Ontology {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, axiom: Axiom) -> usize {
        self.axioms.push(axiom);
        self.axioms.len() - 1
    }

    pub fn with_axiom(mut self, axiom: Axiom) -> Self {
        self.push(axiom);
        self
    }

    pub fn extend<I: IntoIterator<Item = Axiom>>(&mut self, iter: I) {
        self.axioms.extend(iter);
    }

    pub fn axioms(&self) -> &[Axiom] {
        &self.axioms
    }

    pub fn len(&self) -> usize {
        self.axioms.len()
    }

    pub fn is_empty(&self) -> bool {
        self.axioms.is_empty()
    }

    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        serde_json::from_str(json)
    }

    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }

    pub fn to_json_pretty(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }
}

impl std::iter::Extend<Axiom> for Ontology {
    fn extend<I: IntoIterator<Item = Axiom>>(&mut self, iter: I) {
        Ontology::extend(self, iter);
    }
}

impl FromIterator<Axiom> for Ontology {
    fn from_iter<I: IntoIterator<Item = Axiom>>(iter: I) -> Self {
        Self {
            axioms: iter.into_iter().collect(),
        }
    }
}
