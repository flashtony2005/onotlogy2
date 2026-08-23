//! Deterministic consistency checks for the asserted data fragment.
//!
//! The selected OWL backend does not yet provide a datatype-complete tableau,
//! so negative assertions, functional conflicts, range mismatches and disjoint
//! data properties must be checked explicitly before ontology persistence.

use crate::error::{KnowledgeError, Result};
use oaip_abi::{canonical_decimal, compile_xsd_pattern_subset};

use crate::model::{
    Axiom, ClassExpression, DataFacet, DataRange, Datatype, FacetValue, Literal, Ontology, Term,
};
use chrono::{DateTime, NaiveDate, Utc};
use regex::Regex;
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Debug, Clone, PartialEq)]
struct DataFact {
    individual: Term,
    property: Term,
    value: Literal,
}

/// Number of immutable data-consistency conclusions retained across calls.
const DATA_CONSISTENCY_CACHE_CAPACITY: usize = 32;

type DataConsistencyCacheEntry = (u64, Arc<Ontology>, Result<()>);

/// An exact LRU for completed checks of immutable ontology snapshots.
///
/// The fingerprint only locates a candidate; equality on the retained ontology
/// makes a hash collision a miss rather than an unsound hit.
#[derive(Debug)]
struct DataConsistencyLru {
    entries: VecDeque<DataConsistencyCacheEntry>,
    capacity: usize,
}

impl DataConsistencyLru {
    fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            capacity: capacity.max(1),
        }
    }

    fn get(&mut self, key: u64, ontology: &Ontology) -> Option<Result<()>> {
        let position = self
            .entries
            .iter()
            .position(|(candidate, candidate_ontology, _)| {
                *candidate == key && candidate_ontology.as_ref() == ontology
            })?;
        let entry = self.entries.remove(position);
        let (_, cached_ontology, result) = entry.expect("position came from entries");
        self.entries
            .push_back((key, cached_ontology, result.clone()));
        Some(result)
    }

    #[cfg(test)]
    fn contains_key(&self, key: u64, ontology: &Ontology) -> bool {
        self.entries
            .iter()
            .any(|(candidate, candidate_ontology, _)| {
                *candidate == key && candidate_ontology.as_ref() == ontology
            })
    }

    fn put(&mut self, key: u64, ontology: &Ontology, result: Result<()>) {
        self.get(key, ontology);
        self.entries
            .push_back((key, Arc::new(ontology.clone()), result));
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

fn data_consistency_cache() -> &'static Mutex<DataConsistencyLru> {
    static CACHE: OnceLock<Mutex<DataConsistencyLru>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(DataConsistencyLru::new(DATA_CONSISTENCY_CACHE_CAPACITY)))
}

fn ontology_fingerprint(ontology: &Ontology) -> u64 {
    let mut hasher = DefaultHasher::new();
    ontology.axioms().hash(&mut hasher);
    hasher.finish()
}

fn literal_equal(left: &Literal, right: &Literal) -> bool {
    match (left, right) {
        (Literal::Boolean(left), Literal::Boolean(right)) => left == right,
        (Literal::Integer(left), Literal::Integer(right)) => left == right,
        (Literal::String(left), Literal::String(right)) => left == right,
        // Exact IEEE comparison intentionally treats NaN as non-identical.
        (Literal::Double(left), Literal::Double(right)) => left == right,
        (Literal::Decimal(left), Literal::Decimal(right)) => canonical_decimal(left)
            .zip(canonical_decimal(right))
            .is_some_and(|(left, right)| left == right),
        (Literal::Date(left), Literal::Date(right)) => date_value(left)
            .zip(date_value(right))
            .is_some_and(|(left, right)| left == right),
        (Literal::DateTime(left), Literal::DateTime(right)) => date_time_value(left)
            .zip(date_time_value(right))
            .is_some_and(|(left, right)| left == right),
        _ => false,
    }
}

fn date_value(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok()
        .filter(|parsed| parsed.to_string() == value)
}

fn date_time_value(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

fn validate_literal(property: &Term, value: &Literal) -> Result<()> {
    if let Literal::Decimal(value) = value
        && canonical_decimal(value).as_deref() != Some(value.as_str())
    {
        return Err(KnowledgeError::Invalid(format!(
            "data property `{property}` has a non-decimal value {value:?}"
        )));
    }
    if let Literal::Date(value) = value
        && date_value(value).is_none()
    {
        return Err(KnowledgeError::Invalid(format!(
            "data property `{property}` has a non-calendar date value {value:?}"
        )));
    }
    if let Literal::DateTime(value) = value
        && DateTime::parse_from_rfc3339(value).is_err()
    {
        return Err(KnowledgeError::Invalid(format!(
            "data property `{property}` has a non-RFC 3339 dateTime value {value:?}"
        )));
    }
    Ok(())
}

fn literal_datatype_matches(value: &Literal, datatype: Datatype) -> bool {
    value.datatype() == datatype
}

/// Validate a pattern with the one XSD-subset compiler shared by ODL and DL.
fn validate_pattern(property: &Term, pattern: &str) -> Result<Regex> {
    compile_xsd_pattern_subset(pattern).map_err(|error| {
        KnowledgeError::Invalid(format!(
            "data property `{property}` has an unsupported or invalid pattern {pattern:?}: {error}"
        ))
    })
}

/// Conservative structural and semantic bounds for composable data ranges.
const MAX_DATA_RANGE_NODES: usize = 64;
const MAX_DATA_RANGE_DEPTH: usize = 8;
const MAX_DATA_RANGE_TERMS: usize = 64;
const MAX_RESTRICTION_FACETS: usize = 64;

/// Validate syntax and declared facet values before admission checking.
pub(crate) fn validate_data_range(range: &DataRange, property: &Term) -> Result<usize> {
    validate_data_range_inner(range, property, 0)
}

fn validate_data_range_inner(range: &DataRange, property: &Term, depth: usize) -> Result<usize> {
    if depth > MAX_DATA_RANGE_DEPTH {
        return Err(KnowledgeError::Invalid(format!(
            "data property `{property}` range exceeds nesting depth {MAX_DATA_RANGE_DEPTH}"
        )));
    }
    let nodes = match range {
        DataRange::Datatype(_) => 1,
        DataRange::OneOf(values) => {
            if values.len() > MAX_DATA_RANGE_TERMS {
                return Err(KnowledgeError::Invalid(format!(
                    "data property `{property}` enumeration exceeds {MAX_DATA_RANGE_TERMS} terms"
                )));
            }
            if values.is_empty() {
                return Err(KnowledgeError::Invalid(format!(
                    "data property `{property}` has an empty enumeration range"
                )));
            }
            for value in values {
                validate_literal(property, value)?;
            }
            values.len()
        }
        DataRange::Restriction { datatype, facets } => {
            validate_restriction(property, datatype, facets)?;
            1
        }
        DataRange::Complement(child) => 1 + validate_data_range_inner(child, property, depth + 1)?,
        DataRange::Union(branches) | DataRange::Intersection(branches) => {
            if branches.len() < 2 {
                return Err(KnowledgeError::Invalid(format!(
                    "data property `{property}` boolean data range needs at least two branches"
                )));
            }
            let mut total = 1;
            for branch in branches {
                total += validate_data_range_inner(branch, property, depth + 1)?;
                if total > MAX_DATA_RANGE_NODES {
                    break;
                }
            }
            total
        }
    };
    if nodes > MAX_DATA_RANGE_NODES {
        return Err(KnowledgeError::Invalid(format!(
            "data property `{property}` range exceeds {MAX_DATA_RANGE_NODES} nodes"
        )));
    }
    Ok(nodes)
}

fn restriction_admits(value: &Literal, datatype: &Datatype, facets: &[FacetValue]) -> bool {
    literal_datatype_matches(value, *datatype)
        && facets
            .iter()
            .all(|item| facet_admits(value, item.facet, &item.value))
}

/// Whether an asserted typed literal belongs to the denoted data range.
pub(crate) fn data_range_admits(range: &DataRange, value: &Literal) -> bool {
    match range {
        DataRange::Datatype(datatype) => literal_datatype_matches(value, *datatype),
        DataRange::OneOf(values) => values.iter().any(|allowed| literal_equal(value, allowed)),
        DataRange::Restriction { datatype, facets } => restriction_admits(value, datatype, facets),
        DataRange::Complement(child) => !data_range_admits(child, value),
        DataRange::Union(branches) => branches
            .iter()
            .any(|branch| data_range_admits(branch, value)),
        DataRange::Intersection(branches) => branches
            .iter()
            .all(|branch| data_range_admits(branch, value)),
    }
}

fn validate_restriction(
    property: &Term,
    restriction: &Datatype,
    facets: &[FacetValue],
) -> Result<Option<Regex>> {
    let allowed_facets = |facet: DataFacet| match restriction {
        Datatype::String => matches!(
            facet,
            DataFacet::MinLength | DataFacet::MaxLength | DataFacet::Pattern
        ),
        Datatype::Integer | Datatype::Double | Datatype::Date | Datatype::DateTime => {
            matches!(
                facet,
                DataFacet::MinInclusive
                    | DataFacet::MinExclusive
                    | DataFacet::MaxInclusive
                    | DataFacet::MaxExclusive
            )
        }
        Datatype::Decimal => {
            matches!(
                facet,
                DataFacet::MinInclusive
                    | DataFacet::MinExclusive
                    | DataFacet::MaxInclusive
                    | DataFacet::MaxExclusive
                    | DataFacet::TotalDigits
                    | DataFacet::FractionDigits
            )
        }
        Datatype::Boolean => false,
    };
    if !matches!(
        restriction,
        Datatype::String
            | Datatype::Integer
            | Datatype::Double
            | Datatype::Date
            | Datatype::DateTime
            | Datatype::Decimal
    ) {
        return Err(KnowledgeError::Invalid(format!(
            "data property `{property}` restricts unsupported range {restriction:?}"
        )));
    }
    if facets.len() > MAX_RESTRICTION_FACETS {
        return Err(KnowledgeError::Invalid(format!(
            "data property `{property}` restriction exceeds {MAX_RESTRICTION_FACETS} facets"
        )));
    }
    let mut seen = BTreeSet::new();
    for item in facets {
        if !seen.insert(item.facet) {
            return Err(KnowledgeError::Invalid(format!(
                "data property `{property}` repeats facet {:?}",
                item.facet
            )));
        }
        if !allowed_facets(item.facet) {
            return Err(KnowledgeError::Invalid(format!(
                "data property `{property}` does not support facet {:?} for {restriction:?}",
                item.facet
            )));
        }
        let valid_value = match (item.facet, &item.value) {
            (
                DataFacet::MinInclusive
                | DataFacet::MinExclusive
                | DataFacet::MaxInclusive
                | DataFacet::MaxExclusive,
                value,
            ) => literal_datatype_matches(value, *restriction),
            (DataFacet::TotalDigits | DataFacet::FractionDigits, Literal::Integer(length)) => {
                *length >= 0
            }
            (DataFacet::MinLength | DataFacet::MaxLength, Literal::Integer(length)) => *length >= 0,
            (DataFacet::Pattern, Literal::String(_)) => true,
            _ => false,
        };
        if !valid_value {
            return Err(KnowledgeError::Invalid(format!(
                "data property `{property}` facet {:?} has an invalid bound for {restriction:?}",
                item.facet
            )));
        }
        validate_literal(property, &item.value)?;
    }

    facets
        .iter()
        .find_map(|item| match item {
            FacetValue {
                facet: DataFacet::Pattern,
                value: Literal::String(pattern),
            } => Some(validate_pattern(property, pattern)),
            _ => None,
        })
        .transpose()
}

/// Return sign, integral digits, and fractional digits of a canonical decimal.
fn decimal_parts(value: &str) -> Option<(bool, String, String)> {
    let canonical = canonical_decimal(value)?;
    let body = canonical.strip_prefix('-').unwrap_or(&canonical);
    let negative = canonical.starts_with('-');
    let (integer, fraction) = body.split_once('.').unwrap_or((body, ""));
    Some((negative, integer.to_owned(), fraction.to_owned()))
}

fn decimal_total_digits(value: &str) -> Option<usize> {
    let (_, integer, fraction) = decimal_parts(value)?;
    if integer == "0" && fraction.is_empty() {
        return Some(0);
    }
    Some(integer.len() + fraction.len())
}

fn decimal_fraction_digits(value: &str) -> Option<usize> {
    let (_, _, fraction) = decimal_parts(value)?;
    Some(fraction.len())
}

fn compare_decimals(left: &str, right: &str) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering::*;
    let (left_negative, left_integer, left_fraction) = decimal_parts(left)?;
    let (right_negative, right_integer, right_fraction) = decimal_parts(right)?;

    // Canonical zero has no sign.
    let magnitude = {
        let compare_integers = left_integer
            .len()
            .cmp(&right_integer.len())
            .then(left_integer.cmp(&right_integer));
        compare_integers.then_with(|| {
            let common = left_fraction.len().min(right_fraction.len());
            let ordering = left_fraction[..common].cmp(&right_fraction[..common]);
            if ordering != Equal {
                return ordering;
            }
            left_fraction.len().cmp(&right_fraction.len())
        })
    };
    if left_negative != right_negative {
        return Some(if left_negative { Less } else { Greater });
    }
    let unsigned = if left_negative {
        magnitude.reverse()
    } else {
        magnitude
    };
    Some(if unsigned == Equal { Equal } else { unsigned })
}

fn string_length(value: &str) -> usize {
    value.chars().count()
}

fn compare_literals(left: &Literal, right: &Literal) -> Option<std::cmp::Ordering> {
    match (left, right) {
        (Literal::Integer(left), Literal::Integer(right)) => Some(left.cmp(right)),
        (Literal::Double(left), Literal::Double(right)) => left.partial_cmp(right),
        (Literal::Decimal(left), Literal::Decimal(right)) => compare_decimals(left, right),
        (Literal::Date(left), Literal::Date(right)) => {
            date_value(left)?.partial_cmp(&date_value(right)?)
        }
        (Literal::DateTime(left), Literal::DateTime(right)) => {
            date_time_value(left)?.partial_cmp(&date_time_value(right)?)
        }
        _ => None,
    }
}

fn facet_admits(fact: &Literal, facet: DataFacet, bound: &Literal) -> bool {
    if let (DataFacet::Pattern, Literal::String(value), Literal::String(pattern)) =
        (facet, fact, bound)
        && compile_xsd_pattern_subset(pattern).is_ok_and(|regex| regex.is_match(value))
    {
        return true;
    }

    if matches!(facet, DataFacet::MinLength | DataFacet::MaxLength)
        && let Literal::String(value) = fact
        && let Literal::Integer(length) = bound
    {
        let length = *length as usize;
        return match facet {
            DataFacet::MinLength => string_length(value) >= length,
            _ => string_length(value) <= length,
        };
    }

    if matches!(facet, DataFacet::TotalDigits | DataFacet::FractionDigits)
        && let (Literal::Decimal(value), Literal::Integer(bound)) = (fact, bound)
    {
        let count = match facet {
            DataFacet::TotalDigits => decimal_total_digits(value),
            _ => decimal_fraction_digits(value),
        };
        return count.is_some_and(|count| count <= *bound as usize);
    }

    let Some(ordering) = compare_literals(fact, bound) else {
        return false;
    };
    use std::cmp::Ordering::*;
    match facet {
        DataFacet::MinInclusive => ordering != Less,
        DataFacet::MinExclusive => ordering == Greater,
        DataFacet::MaxInclusive => ordering != Greater,
        DataFacet::MaxExclusive => ordering == Less,
        DataFacet::MinLength | DataFacet::MaxLength => unreachable!("handled above"),
        DataFacet::TotalDigits | DataFacet::FractionDigits | DataFacet::Pattern => {
            unreachable!("handled above")
        }
    }
}

pub(crate) fn parents(map: &mut BTreeMap<Term, Term>, mut term: Term) -> Term {
    let mut root = term.clone();
    while let Some(parent) = map.get(&root) {
        let parent = parent.clone();
        if parent == root {
            break;
        }
        root = parent;
    }
    // Path compression keeps the small ABox case linear after first touch.
    while term != root {
        let next = map.get(&term).cloned().unwrap_or_else(|| root.clone());
        map.insert(term.clone(), root.clone());
        term = next;
    }
    root
}

/// Return the property plus every transitive super-property.  A positive assertion
/// on a sub-property entails the same assertion on each of its ancestors.
pub(crate) fn property_ancestors(edges: &BTreeMap<Term, Term>, property: &Term) -> BTreeSet<Term> {
    let mut found = BTreeSet::new();
    let mut queue = vec![property.clone()];
    while let Some(current) = queue.pop() {
        if !found.insert(current.clone()) {
            continue;
        }
        if let Some(parent) = edges.get(&current) {
            queue.push(parent.clone());
        }
    }
    found
}

pub(crate) fn union(map: &mut BTreeMap<Term, Term>, left: &Term, right: &Term) {
    let left_root = parents(map, left.clone());
    let right_root = parents(map, right.clone());
    if left_root == right_root {
        return;
    }
    if left_root <= right_root {
        map.insert(right_root, left_root);
    } else {
        map.insert(left_root, right_root);
    }
}

/// Materialize the OWL data-property-domain consequences that the selected
/// tableau does not propagate.  Domain assertions are derived, never written
/// back into the governed source ontology.
pub(crate) fn materialize_data_domain_assertions(ontology: &Ontology) -> Ontology {
    let mut aliases = BTreeMap::new();
    let mut sub_data_properties = BTreeMap::new();
    let mut domains = BTreeMap::<Term, Vec<ClassExpression>>::new();
    let mut positive = Vec::new();

    for axiom in ontology.axioms() {
        match axiom {
            Axiom::SameIndividuals(terms) if terms.len() >= 2 => {
                for pair in terms.windows(2) {
                    union(&mut aliases, &pair[0], &pair[1]);
                }
            }
            Axiom::SubDataPropertyOf { sub, sup } => {
                sub_data_properties.insert(sub.clone(), sup.clone());
            }
            Axiom::DataPropertyDomain { property, class } => {
                let classes = domains.entry(property.clone()).or_default();
                if !classes.contains(class) {
                    classes.push(class.clone());
                }
            }
            Axiom::DataPropertyAssertion {
                property,
                individual,
                value,
            } => positive.push(DataFact {
                individual: individual.clone(),
                property: property.clone(),
                value: value.clone(),
            }),
            _ => {}
        }
    }

    let mut materialized = ontology.clone();
    for fact in &positive {
        let individual = parents(&mut aliases, fact.individual.clone());
        for property in property_ancestors(&sub_data_properties, &fact.property) {
            let Some(domain_classes) = domains.get(&property) else {
                continue;
            };
            for class in domain_classes {
                materialized.push(Axiom::ClassAssertion {
                    class: class.clone(),
                    individual: individual.clone(),
                });
            }
        }
    }
    materialized
}

/// Validate the asserted data fragment that is not yet covered soundly by the
/// external tableau.  The checks are conservative for the supported vocabulary.
pub fn validate_data_consistency(ontology: &Ontology) -> Result<()> {
    let key = ontology_fingerprint(ontology);
    let mut cache = data_consistency_cache()
        .lock()
        .expect("data consistency cache mutex poisoned");
    if let Some(result) = cache.get(key, ontology) {
        return result;
    }

    let result = validate_data_consistency_uncached(ontology);
    cache.put(key, ontology, result.clone());
    result
}

fn validate_data_consistency_uncached(ontology: &Ontology) -> Result<()> {
    let mut aliases = BTreeMap::new();
    let mut positive = Vec::new();
    let mut negative = Vec::new();
    let mut functional = BTreeSet::new();
    let mut disjoint = BTreeSet::new();
    let mut ranges = BTreeMap::<Term, BTreeSet<Datatype>>::new();
    let mut one_ofs = BTreeMap::<Term, Vec<Vec<Literal>>>::new();
    let mut restrictions =
        BTreeMap::<Term, Vec<(usize, Datatype, Vec<FacetValue>, Option<Regex>)>>::new();
    let mut restriction_count = 0usize;
    let mut range_expressions = BTreeMap::<Term, Vec<DataRange>>::new();
    let mut sub_data_properties = BTreeMap::new();

    for axiom in ontology.axioms() {
        match axiom {
            Axiom::SameIndividuals(terms) if terms.len() >= 2 => {
                for pair in terms.windows(2) {
                    union(&mut aliases, &pair[0], &pair[1]);
                }
            }
            Axiom::DataPropertyAssertion {
                property,
                individual,
                value,
            } => positive.push(DataFact {
                individual: individual.clone(),
                property: property.clone(),
                value: value.clone(),
            }),
            Axiom::NegativeDataPropertyAssertion {
                property,
                individual,
                value,
            } => negative.push(DataFact {
                individual: individual.clone(),
                property: property.clone(),
                value: value.clone(),
            }),
            Axiom::FunctionalDataProperty(property) => {
                functional.insert(property.clone());
            }
            Axiom::DisjointDataProperties(properties) if properties.len() >= 2 => {
                for left in 0..properties.len() {
                    for right in left + 1..properties.len() {
                        let first = &properties[left];
                        let second = &properties[right];
                        disjoint.insert((first.min(second).clone(), first.max(second).clone()));
                    }
                }
            }
            Axiom::DataPropertyRange { property, datatype } => {
                ranges
                    .entry(property.clone())
                    .or_default()
                    .insert(*datatype);
            }
            Axiom::DataPropertyOneOf { property, values } => {
                one_ofs
                    .entry(property.clone())
                    .or_default()
                    .push(values.clone());
            }
            Axiom::DataPropertyRangeRestriction {
                property,
                datatype,
                facets,
            } => {
                let pattern = validate_restriction(property, datatype, facets)?;
                restrictions.entry(property.clone()).or_default().push((
                    restriction_count,
                    *datatype,
                    facets.clone(),
                    pattern,
                ));
                restriction_count += 1;
            }
            Axiom::DataPropertyDataRange { property, range } => {
                validate_data_range(range, property)?;
                range_expressions
                    .entry(property.clone())
                    .or_default()
                    .push(range.clone());
            }
            Axiom::SubDataPropertyOf { sub, sup } => {
                sub_data_properties.insert(sub.clone(), sup.clone());
            }
            _ => {}
        }
    }

    // Materialize entailments through the data-property hierarchy before the
    // deterministic checks below.  Both polarities propagate upward: if
    // `work-email ⊑ email`, then positive/negative facts about `work-email`
    // entail the same polarity about `email`.
    let expand = |facts: &[DataFact]| -> Vec<DataFact> {
        facts
            .iter()
            .flat_map(|fact| {
                property_ancestors(&sub_data_properties, &fact.property)
                    .into_iter()
                    .map(move |property| DataFact {
                        individual: fact.individual.clone(),
                        property,
                        value: fact.value.clone(),
                    })
            })
            .collect()
    };
    let positive = expand(&positive);
    let negative = expand(&negative);

    for fact in positive.iter().chain(negative.iter()) {
        validate_literal(&fact.property, &fact.value)?;
    }

    // Index negatives by canonical individual and property so a large ABox does
    // not pay an O(assertions x negative-assertions) contradiction scan.
    let mut negatives = BTreeMap::<(Term, Term), Vec<&DataFact>>::new();
    for fact in &negative {
        let individual = parents(&mut aliases, fact.individual.clone());
        negatives
            .entry((individual, fact.property.clone()))
            .or_default()
            .push(fact);
    }

    // Two different concrete ranges make every assertion violate at least one
    // of them under the current basic-datatype vocabulary.
    for (property, datatypes) in &ranges {
        if datatypes.len() > 1 {
            return Err(KnowledgeError::Invalid(format!(
                "data property `{property}` has contradictory ranges: {datatypes:?}"
            )));
        }
    }

    let mut functional_values = BTreeMap::<(Term, Term), Literal>::new();
    let mut properties_by_individual = BTreeMap::<Term, BTreeSet<Term>>::new();
    for fact in &positive {
        let individual = parents(&mut aliases, fact.individual.clone());

        if let Some(candidate_ranges) = range_expressions.get(&fact.property) {
            for range in candidate_ranges {
                if !data_range_admits(range, &fact.value) {
                    return Err(KnowledgeError::Invalid(format!(
                        "data property `{}` range rejects `{individual}` value {:?}",
                        fact.property, fact.value
                    )));
                }
            }
        }

        let violated_range = ranges
            .get(&fact.property)
            .is_some_and(|allowed| !allowed.contains(&fact.value.datatype()));
        if violated_range {
            return Err(KnowledgeError::Invalid(format!(
                "data property `{}` range rejects `{individual}` value {:?}",
                fact.property, fact.value
            )));
        }

        if let Some(matching) = restrictions.get(&fact.property) {
            let mut violated_restriction = None;
            for (restriction_index, datatype, facets, pattern) in matching {
                let facets_admit = facets
                    .iter()
                    .all(|item| facet_admits(&fact.value, item.facet, &item.value));
                let pattern_admits = pattern.as_ref().is_none_or(|regex| {
                    let Literal::String(value) = &fact.value else {
                        return false;
                    };
                    regex.is_match(value)
                });
                if datatype == &fact.value.datatype() && (!facets_admit || !pattern_admits) {
                    violated_restriction = Some(*restriction_index);
                    break;
                }
            }
            if let Some(restriction_index) = violated_restriction {
                return Err(KnowledgeError::Invalid(format!(
                    "data property `{}` restriction #{restriction_index} rejects `{individual}` value {:?}",
                    fact.property, fact.value
                )));
            }
        }

        if let Some(enumerations) = one_ofs.get(&fact.property) {
            let rejected = enumerations.iter().any(|allowed| {
                !allowed
                    .iter()
                    .any(|value| literal_equal(value, &fact.value))
            });
            if rejected {
                return Err(KnowledgeError::Invalid(format!(
                    "data property `{}` enumeration rejects `{individual}` value {:?}",
                    fact.property, fact.value
                )));
            }
        }

        let contradicted = negatives
            .get(&(individual.clone(), fact.property.clone()))
            .is_some_and(|candidates| {
                candidates
                    .iter()
                    .any(|negative| literal_equal(&negative.value, &fact.value))
            });
        if contradicted {
            return Err(KnowledgeError::Invalid(format!(
                "individual `{individual}` has positive and negative assertions for data property `{}`",
                fact.property
            )));
        }

        let properties = properties_by_individual
            .entry(individual.clone())
            .or_default();
        if properties.insert(fact.property.clone()) {
            for existing in properties.iter() {
                if *existing == fact.property {
                    continue;
                }
                let pair = if existing <= &fact.property {
                    (existing.clone(), fact.property.clone())
                } else {
                    (fact.property.clone(), existing.clone())
                };
                if disjoint.contains(&pair) {
                    return Err(KnowledgeError::Invalid(format!(
                        "data properties `{}` and `{}` are disjoint but both are asserted for `{individual}`",
                        pair.0, pair.1
                    )));
                }
            }
        }

        if !functional.contains(&fact.property) {
            continue;
        }
        let key = (individual, fact.property.clone());
        match functional_values.get(&key) {
            Some(existing) if literal_equal(existing, &fact.value) => {}
            Some(_) => {
                return Err(KnowledgeError::Invalid(format!(
                    "functional data property `{}` has conflicting values for `{}`",
                    fact.property, key.0
                )));
            }
            None => {
                functional_values.insert(key, fact.value.clone());
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Reasoner;

    fn term(value: &str) -> Term {
        Term::new(value)
    }

    fn fact(individual: &str, property: &str, value: Literal) -> Axiom {
        Axiom::DataPropertyAssertion {
            property: term(property),
            individual: term(individual),
            value,
        }
    }

    fn negative(individual: &str, property: &str, value: Literal) -> Axiom {
        Axiom::NegativeDataPropertyAssertion {
            property: term(property),
            individual: term(individual),
            value,
        }
    }

    #[test]
    fn accepts_repeated_and_distinct_nonfunctional_values() {
        let ontology = Ontology::new()
            .with_axiom(fact("a", "tag", Literal::String("x".into())))
            .with_axiom(fact("a", "tag", Literal::String("x".into())))
            .with_axiom(fact("a", "note", Literal::String("y".into())));
        assert!(validate_data_consistency(&ontology).is_ok());
    }

    #[test]
    fn rejects_positive_negative_and_functional_conflicts() {
        let contradictory = Ontology::new()
            .with_axiom(fact("a", "age", Literal::Integer(30)))
            .with_axiom(negative("a", "age", Literal::Integer(30)));
        assert!(validate_data_consistency(&contradictory).is_err());

        let functional = Ontology::new()
            .with_axiom(Axiom::FunctionalDataProperty(term("age")))
            .with_axiom(fact("a", "age", Literal::Integer(30)))
            .with_axiom(fact("a", "age", Literal::Integer(31)));
        assert!(validate_data_consistency(&functional).is_err());
    }

    #[test]
    fn large_abox_uses_indexed_consistency_checks() {
        let ontology = (0..1_000).fold(Ontology::new(), |ontology, index| {
            if index == 0 {
                ontology
                    .with_axiom(Axiom::FunctionalDataProperty(term("status")))
                    .with_axiom(fact(
                        &format!("individual-{index}"),
                        "status",
                        Literal::String("active".into()),
                    ))
            } else {
                ontology.with_axiom(fact(
                    &format!("individual-{index}"),
                    "status",
                    Literal::String("active".into()),
                ))
            }
        });
        assert!(validate_data_consistency(&ontology).is_ok());

        let conflict = ontology.with_axiom(fact(
            "individual-999",
            "status",
            Literal::String("archived".into()),
        ));
        assert!(validate_data_consistency(&conflict).is_err());
    }

    #[test]
    fn repeated_consistency_calls_reuse_the_exact_result() {
        let ontology = Ontology::new().with_axiom(fact(
            "cache-candidate",
            "status",
            Literal::String("active".into()),
        ));
        let key = ontology_fingerprint(&ontology);

        assert!(
            !data_consistency_cache()
                .lock()
                .expect("cache mutex")
                .contains_key(key, &ontology)
        );
        assert!(validate_data_consistency(&ontology).is_ok());
        assert!(
            data_consistency_cache()
                .lock()
                .expect("cache mutex")
                .get(key, &ontology)
                == Some(Ok(()))
        );
        assert!(validate_data_consistency(&ontology).is_ok());
    }

    #[test]
    fn consistency_errors_are_reused_without_becoming_successes() {
        let conflict = Ontology::new()
            .with_axiom(Axiom::FunctionalDataProperty(term("age")))
            .with_axiom(fact("cache-conflict", "age", Literal::Integer(30)))
            .with_axiom(fact("cache-conflict", "age", Literal::Integer(31)));
        let key = ontology_fingerprint(&conflict);

        assert!(validate_data_consistency(&conflict).is_err());
        assert!(validate_data_consistency(&conflict).is_err());
        assert!(
            data_consistency_cache()
                .lock()
                .expect("cache mutex")
                .get(key, &conflict)
                .is_some_and(|result| result.is_err())
        );
    }

    #[test]
    fn fingerprint_collision_is_treated_as_a_cache_miss() {
        let retained = Ontology::new().with_axiom(fact("a", "tag", Literal::String("kept".into())));
        let candidate =
            Ontology::new().with_axiom(fact("a", "tag", Literal::String("other".into())));
        let mut cache = DataConsistencyLru::new(1);

        cache.put(42, &retained, Ok(()));
        assert!(cache.get(42, &candidate).is_none());
        cache.put(42, &candidate, Ok(()));

        assert_eq!(1, cache.len());
        assert!(!cache.contains_key(42, &retained));
        assert_eq!(Some(Ok(())), cache.get(42, &candidate));
    }

    #[test]
    fn signed_zero_literals_hash_as_equal_candidates() {
        let positive = Ontology::new().with_axiom(fact("sensor", "reading", Literal::Double(0.0)));
        let negative = Ontology::new().with_axiom(fact("sensor", "reading", Literal::Double(-0.0)));

        assert_eq!(positive, negative);
        assert_eq!(
            ontology_fingerprint(&positive),
            ontology_fingerprint(&negative)
        );
    }

    #[test]
    fn date_times_compare_by_instant_and_invalid_values_fail_closed() {
        let equivalent = Ontology::new()
            .with_axiom(Axiom::FunctionalDataProperty(term("occurred-at")))
            .with_axiom(fact(
                "a",
                "occurred-at",
                Literal::DateTime("2026-08-23T08:00:00+08:00".into()),
            ))
            .with_axiom(fact(
                "a",
                "occurred-at",
                Literal::DateTime("2026-08-23T00:00:00Z".into()),
            ));
        assert!(validate_data_consistency(&equivalent).is_ok());

        let malformed = Ontology::new().with_axiom(fact(
            "a",
            "occurred-at",
            Literal::DateTime("tomorrow".into()),
        ));
        assert!(validate_data_consistency(&malformed).is_err());
    }

    #[test]
    fn same_individual_aliases_participate_in_conflicts() {
        let ontology = Ontology::new()
            .with_axiom(Axiom::SameIndividuals(vec![term("a"), term("b")]))
            .with_axiom(fact("a", "age", Literal::Integer(30)))
            .with_axiom(negative("b", "age", Literal::Integer(30)));
        assert!(validate_data_consistency(&ontology).is_err());
    }

    #[test]
    fn subproperty_negative_and_parent_positive_conflict() {
        let ontology = Ontology::new()
            .with_axiom(Axiom::SubDataPropertyOf {
                sub: term("work-email"),
                sup: term("email"),
            })
            .with_axiom(negative("a", "work-email", Literal::String("a@x".into())))
            .with_axiom(fact("a", "email", Literal::String("a@x".into())));
        assert!(validate_data_consistency(&ontology).is_err());
    }

    #[test]
    fn parent_negative_and_subproperty_positive_conflict() {
        let ontology = Ontology::new()
            .with_axiom(Axiom::SubDataPropertyOf {
                sub: term("work-email"),
                sup: term("email"),
            })
            .with_axiom(negative("a", "email", Literal::String("a@x".into())))
            .with_axiom(fact("a", "work-email", Literal::String("a@x".into())));
        assert!(validate_data_consistency(&ontology).is_err());
    }

    #[test]
    fn rejects_range_disjointness_and_multiple_ranges() {
        let range = Ontology::new()
            .with_axiom(Axiom::DataPropertyRange {
                property: term("age"),
                datatype: Datatype::Integer,
            })
            .with_axiom(fact("a", "age", Literal::String("old".into())));
        assert!(validate_data_consistency(&range).is_err());

        let disjoint = Ontology::new()
            .with_axiom(Axiom::DisjointDataProperties(vec![
                term("mobile"),
                term("landline"),
            ]))
            .with_axiom(fact("a", "mobile", Literal::String("1".into())))
            .with_axiom(fact("a", "landline", Literal::String("2".into())));
        assert!(validate_data_consistency(&disjoint).is_err());

        let multiple_ranges = Ontology::new()
            .with_axiom(Axiom::DataPropertyRange {
                property: term("value"),
                datatype: Datatype::Integer,
            })
            .with_axiom(Axiom::DataPropertyRange {
                property: term("value"),
                datatype: Datatype::String,
            });
        assert!(validate_data_consistency(&multiple_ranges).is_err());
    }

    #[test]
    fn datatype_restrictions_fail_closed() {
        fn age_restriction(facets: Vec<FacetValue>) -> Axiom {
            Axiom::DataPropertyRangeRestriction {
                property: term("age"),
                datatype: Datatype::Integer,
                facets,
            }
        }
        let integer = |value: i64| Literal::Integer(value);
        let facet = |facet: DataFacet, value: i64| FacetValue {
            facet,
            value: integer(value),
        };
        let age_fact = |value: i64| fact("a", "age", integer(value));

        let bounds = age_restriction(vec![
            facet(DataFacet::MinExclusive, 17),
            facet(DataFacet::MaxInclusive, 65),
        ]);
        let valid = Ontology::new()
            .with_axiom(bounds.clone())
            .with_axiom(age_fact(18));
        assert!(validate_data_consistency(&valid).is_ok());
        let boundary = Ontology::new()
            .with_axiom(bounds.clone())
            .with_axiom(age_fact(65));
        assert!(validate_data_consistency(&boundary).is_ok());

        let outside = Ontology::new().with_axiom(bounds).with_axiom(age_fact(17));
        assert!(validate_data_consistency(&outside).is_err());
        let reasoner = Reasoner::new(&outside).unwrap();
        assert!(!reasoner.is_consistent().unwrap());
        let justification = reasoner.explain_inconsistency().unwrap().unwrap();
        assert!(
            justification
                .axioms
                .iter()
                .any(|axiom| matches!(axiom, Axiom::DataPropertyRangeRestriction { .. }))
        );

        // Multiple ranges intersect, so this accepts only [20,30].
        let intersecting = Ontology::new()
            .with_axiom(age_restriction(vec![facet(DataFacet::MinInclusive, 10)]))
            .with_axiom(age_restriction(vec![
                facet(DataFacet::MinInclusive, 20),
                facet(DataFacet::MaxInclusive, 30),
            ]))
            .with_axiom(age_fact(25));
        assert!(validate_data_consistency(&intersecting).is_ok());

        let malformed = age_restriction(vec![FacetValue {
            facet: DataFacet::MinInclusive,
            value: Literal::String("young".into()),
        }]);
        assert!(validate_data_consistency(&Ontology::new().with_axiom(malformed)).is_err());

        // XSD dateTimes compare by instant, not by lexical offset.
        let date_bounds = Axiom::DataPropertyRangeRestriction {
            property: term("occurred-at"),
            datatype: Datatype::DateTime,
            facets: vec![FacetValue {
                facet: DataFacet::MinExclusive,
                value: Literal::DateTime("2026-08-23T00:00:00Z".into()),
            }],
        };
        fn make_date_fact(value: &str) -> Axiom {
            fact("a", "occurred-at", Literal::DateTime(value.into()))
        }
        let same_instant = Ontology::new()
            .with_axiom(date_bounds.clone())
            .with_axiom(make_date_fact("2026-08-23T08:00:00+08:00"));
        assert!(validate_data_consistency(&same_instant).is_err());
        let after = Ontology::new()
            .with_axiom(date_bounds)
            .with_axiom(make_date_fact("2026-08-23T08:00:01+08:00"));
        assert!(validate_data_consistency(&after).is_ok());
    }

    #[test]
    fn string_length_restrictions_fail_closed() {
        fn name_restriction(min: i64, max: i64) -> Axiom {
            Axiom::DataPropertyRangeRestriction {
                property: term("name"),
                datatype: Datatype::String,
                facets: vec![
                    FacetValue {
                        facet: DataFacet::MinLength,
                        value: Literal::Integer(min),
                    },
                    FacetValue {
                        facet: DataFacet::MaxLength,
                        value: Literal::Integer(max),
                    },
                ],
            }
        }
        let named = |value: &str| fact("a", "name", Literal::String(value.into()));

        let bounds = name_restriction(2, 3);
        let valid = Ontology::new()
            .with_axiom(bounds.clone())
            .with_axiom(named("中文"));
        assert!(validate_data_consistency(&valid).is_ok());

        let outside = Ontology::new()
            .with_axiom(bounds)
            .with_axiom(named("too-long"));
        assert!(validate_data_consistency(&outside).is_err());
        let reasoner = Reasoner::new(&outside).unwrap();
        assert!(!reasoner.is_consistent().unwrap());
        let justification = reasoner.explain_inconsistency().unwrap().unwrap();
        assert!(
            justification
                .axioms
                .iter()
                .any(|axiom| matches!(axiom, Axiom::DataPropertyRangeRestriction { .. }))
        );

        // Length bounds count Unicode scalar values, not bytes.
        let unicode_bounds = Ontology::new()
            .with_axiom(name_restriction(2, 2))
            .with_axiom(named("中文"));
        assert!(validate_data_consistency(&unicode_bounds).is_ok());

        let negative_length = Ontology::new().with_axiom(Axiom::DataPropertyRangeRestriction {
            property: term("name"),
            datatype: Datatype::String,
            facets: vec![FacetValue {
                facet: DataFacet::MinLength,
                value: Literal::Integer(-1),
            }],
        });
        assert!(validate_data_consistency(&negative_length).is_err());

        let wrong_facet_for_type =
            Ontology::new().with_axiom(Axiom::DataPropertyRangeRestriction {
                property: term("age"),
                datatype: Datatype::Integer,
                facets: vec![FacetValue {
                    facet: DataFacet::MinLength,
                    value: Literal::Integer(1),
                }],
            });
        assert!(validate_data_consistency(&wrong_facet_for_type).is_err());
    }

    #[test]
    fn string_patterns_fail_closed_and_intersect() {
        fn pattern_restriction(pattern: &str) -> Axiom {
            Axiom::DataPropertyRangeRestriction {
                property: term("code"),
                datatype: Datatype::String,
                facets: vec![FacetValue {
                    facet: DataFacet::Pattern,
                    value: Literal::String(pattern.into()),
                }],
            }
        }
        let coded = |value: &str| fact("a", "code", Literal::String(value.into()));

        let valid = Ontology::new()
            .with_axiom(pattern_restriction("[A-Z]{2}[0-9]{4}"))
            .with_axiom(coded("AB1234"));
        assert!(validate_data_consistency(&valid).is_ok());

        let outside = Ontology::new()
            .with_axiom(pattern_restriction("[A-Z]{2}[0-9]{4}"))
            .with_axiom(coded("AB123"));
        assert!(validate_data_consistency(&outside).is_err());
        let reasoner = Reasoner::new(&outside).unwrap();
        assert!(!reasoner.is_consistent().unwrap());

        // Separate range restrictions intersect, as required by XSD facets.
        let intersecting = Ontology::new()
            .with_axiom(pattern_restriction("AB.*"))
            .with_axiom(pattern_restriction(".*34"))
            .with_axiom(coded("AB1234"));
        assert!(validate_data_consistency(&intersecting).is_ok());
        let incompatible = intersecting.with_axiom(pattern_restriction("AB123"));
        assert!(validate_data_consistency(&incompatible).is_err());

        let inline_flags = Ontology::new().with_axiom(pattern_restriction("(?i)ab"));
        assert!(validate_data_consistency(&inline_flags).is_err());
        let explicit_anchor = Ontology::new().with_axiom(pattern_restriction("^AB$"));
        assert!(validate_data_consistency(&explicit_anchor).is_err());
        let class_union = Ontology::new().with_axiom(pattern_restriction("[A-Z|[0-9]]"));
        assert!(validate_data_consistency(&class_union).is_err());
        let wrong_bound = Ontology::new().with_axiom(Axiom::DataPropertyRangeRestriction {
            property: term("code"),
            datatype: Datatype::String,
            facets: vec![FacetValue {
                facet: DataFacet::Pattern,
                value: Literal::Integer(1),
            }],
        });
        assert!(validate_data_consistency(&wrong_bound).is_err());
    }

    #[test]
    fn exact_decimals_order_and_restrict_digits_fail_closed() {
        fn balance_restriction(facet: DataFacet, value: impl Into<Literal>) -> Axiom {
            Axiom::DataPropertyRangeRestriction {
                property: term("balance"),
                datatype: Datatype::Decimal,
                facets: vec![FacetValue {
                    facet,
                    value: value.into(),
                }],
            }
        }
        let fact = |value: &str| fact("a", "balance", Literal::Decimal(value.into()));

        // Ordering is lexical arbitrary-width arithmetic, never IEEE f64.
        let bounds = Ontology::new()
            .with_axiom(balance_restriction(
                DataFacet::MinExclusive,
                Literal::Decimal("99999999999999999999.9998".into()),
            ))
            .with_axiom(balance_restriction(
                DataFacet::MaxInclusive,
                Literal::Decimal("99999999999999999999.9999".into()),
            ));
        assert!(
            validate_data_consistency(
                &bounds
                    .clone()
                    .with_axiom(fact("99999999999999999999.99990"))
            )
            .is_err()
        );
        assert!(
            validate_data_consistency(&bounds.with_axiom(fact("99999999999999999999.9999")))
                .is_ok()
        );

        let negatives = Ontology::new()
            .with_axiom(balance_restriction(
                DataFacet::MinInclusive,
                Literal::Decimal("-1.3".into()),
            ))
            .with_axiom(fact("-1.2"));
        assert!(validate_data_consistency(&negatives).is_ok());

        // Canonical zero has no negative sign and no significant digits.
        let zero = Ontology::new().with_axiom(fact("-0.00"));
        assert!(validate_data_consistency(&zero).is_err());
        let zero_canonical = Ontology::new().with_axiom(fact("0"));
        assert!(validate_data_consistency(&zero_canonical).is_ok());

        let digits = Ontology::new()
            .with_axiom(balance_restriction(
                DataFacet::TotalDigits,
                Literal::Integer(4),
            ))
            .with_axiom(balance_restriction(
                DataFacet::FractionDigits,
                Literal::Integer(2),
            ))
            .with_axiom(fact("-12.30"));
        assert!(validate_data_consistency(&digits).is_err());
        let digits = Ontology::new()
            .with_axiom(balance_restriction(
                DataFacet::TotalDigits,
                Literal::Integer(4),
            ))
            .with_axiom(balance_restriction(
                DataFacet::FractionDigits,
                Literal::Integer(2),
            ))
            .with_axiom(fact("-12.30"));
        // The stored literal must already be canonical before facet evaluation.
        assert!(validate_data_consistency(&digits).is_err());

        let malformed_bound = balance_restriction(DataFacet::TotalDigits, Literal::Integer(-1));
        assert!(validate_data_consistency(&Ontology::new().with_axiom(malformed_bound)).is_err());
    }

    #[test]
    fn composable_data_ranges_admit_alternatives_and_intersections() {
        let id_range = DataRange::union([
            DataRange::restriction(
                Datatype::String,
                vec![FacetValue {
                    facet: DataFacet::Pattern,
                    value: Literal::String("USER-[0-9]+".into()),
                }],
            ),
            DataRange::Datatype(Datatype::Integer),
        ]);
        let axiom = |range: DataRange| Axiom::DataPropertyDataRange {
            property: term("id"),
            range,
        };

        let string_match = Ontology::new()
            .with_axiom(axiom(id_range.clone()))
            .with_axiom(fact("a", "id", Literal::String("USER-42".into())));
        assert!(validate_data_consistency(&string_match).is_ok());

        let integer_match = Ontology::new()
            .with_axiom(axiom(id_range.clone()))
            .with_axiom(fact("a", "id", Literal::Integer(42)));
        assert!(validate_data_consistency(&integer_match).is_ok());

        let rejected = Ontology::new().with_axiom(axiom(id_range)).with_axiom(fact(
            "a",
            "id",
            Literal::Boolean(true),
        ));
        assert!(validate_data_consistency(&rejected).is_err());

        let intersection = DataRange::intersection([
            DataRange::Datatype(Datatype::Integer),
            DataRange::restriction(
                Datatype::Integer,
                vec![FacetValue {
                    facet: DataFacet::MinInclusive,
                    value: Literal::Integer(18),
                }],
            ),
        ]);
        let adult = Ontology::new()
            .with_axiom(axiom(intersection.clone()))
            .with_axiom(fact("a", "id", Literal::Integer(21)));
        assert!(validate_data_consistency(&adult).is_ok());
        let child = Ontology::new()
            .with_axiom(axiom(intersection))
            .with_axiom(fact("a", "id", Literal::Integer(17)));
        assert!(validate_data_consistency(&child).is_err());
    }

    #[test]
    fn malformed_and_unbounded_composable_ranges_fail_closed() {
        let singleton = Axiom::DataPropertyDataRange {
            property: term("value"),
            range: DataRange::Union(vec![DataRange::Datatype(Datatype::String)]),
        };
        assert!(validate_data_consistency(&Ontology::new().with_axiom(singleton)).is_err());

        let mut nested = DataRange::Datatype(Datatype::String);
        for _ in 0..(MAX_DATA_RANGE_DEPTH + 1) {
            nested = DataRange::Intersection(vec![nested, DataRange::Datatype(Datatype::String)]);
        }
        let too_deep = Axiom::DataPropertyDataRange {
            property: term("value"),
            range: nested,
        };
        assert!(validate_data_consistency(&Ontology::new().with_axiom(too_deep)).is_err());

        let empty_enumeration = Axiom::DataPropertyDataRange {
            property: term("value"),
            range: DataRange::OneOf(Vec::new()),
        };
        assert!(validate_data_consistency(&Ontology::new().with_axiom(empty_enumeration)).is_err());

        let oversized = Axiom::DataPropertyDataRange {
            property: term("value"),
            range: DataRange::OneOf(
                std::iter::repeat_n(Literal::String("x".into()), MAX_DATA_RANGE_TERMS + 1)
                    .collect(),
            ),
        };
        assert!(validate_data_consistency(&Ontology::new().with_axiom(oversized)).is_err());
    }

    #[test]
    fn calendar_date_restrictions_fail_closed() {
        fn date_restriction(facet: DataFacet, bound: &str) -> Axiom {
            Axiom::DataPropertyRangeRestriction {
                property: term("birthday"),
                datatype: Datatype::Date,
                facets: vec![FacetValue {
                    facet,
                    value: Literal::Date(bound.into()),
                }],
            }
        }
        let fact = |value: &str| fact("a", "birthday", Literal::Date(value.into()));

        let bounds = Ontology::new()
            .with_axiom(date_restriction(DataFacet::MinInclusive, "2024-01-01"))
            .with_axiom(date_restriction(DataFacet::MaxExclusive, "2025-01-01"));
        assert!(validate_data_consistency(&bounds.clone().with_axiom(fact("2024-02-29"))).is_ok());
        assert!(validate_data_consistency(&bounds.clone().with_axiom(fact("2025-01-01"))).is_err());
        assert!(validate_data_consistency(&bounds.with_axiom(fact("2023-12-31"))).is_err());

        // The explicit checker closes the datatype gap before the tableau runs.
        for malformed in ["2026-8-23", "2026-08-23Z"] {
            let malformed_fact = Axiom::DataPropertyAssertion {
                property: term("birthday"),
                individual: term("a"),
                value: Literal::Date(malformed.into()),
            };
            assert!(
                validate_data_consistency(&Ontology::new().with_axiom(malformed_fact)).is_err()
            );
        }

        let malformed_restriction = Axiom::DataPropertyRangeRestriction {
            property: term("birthday"),
            datatype: Datatype::Date,
            facets: vec![FacetValue {
                facet: DataFacet::MinInclusive,
                value: Literal::String("yesterday".into()),
            }],
        };
        assert!(
            validate_data_consistency(&Ontology::new().with_axiom(malformed_restriction)).is_err()
        );
    }

    #[test]
    fn data_property_enumerations_fail_closed() {
        let literal_values = |values: &[&str]| {
            values
                .iter()
                .map(|value| Literal::String((*value).into()))
                .collect::<Vec<_>>()
        };
        let status_one_of = |allowed: &[&str]| Axiom::DataPropertyOneOf {
            property: term("status"),
            values: literal_values(allowed),
        };
        let fact = |value: &str| fact("a", "status", Literal::String(value.into()));

        let valid = Ontology::new()
            .with_axiom(status_one_of(&["Active", "Pending"]))
            .with_axiom(fact("Active"));
        assert!(validate_data_consistency(&valid).is_ok());

        let rejected = Ontology::new()
            .with_axiom(status_one_of(&["Active", "Pending"]))
            .with_axiom(fact("Archived"));
        assert!(validate_data_consistency(&rejected).is_err());
        let reasoner = Reasoner::new(&rejected).unwrap();
        assert!(!reasoner.is_consistent().unwrap());
        let justification = reasoner.explain_inconsistency().unwrap().unwrap();
        assert!(
            justification
                .axioms
                .iter()
                .any(|axiom| matches!(axiom, Axiom::DataPropertyOneOf { .. }))
        );

        // Multiple OWL ranges intersect rather than union.
        let intersection = Ontology::new()
            .with_axiom(status_one_of(&["Active", "Archived"]))
            .with_axiom(status_one_of(&["Active", "Closed"]))
            .with_axiom(fact("Active"));
        assert!(validate_data_consistency(&intersection).is_ok());
        let excluded = Ontology::from_iter(
            intersection
                .axioms()
                .iter()
                .take(2)
                .cloned()
                .chain([fact("Archived")]),
        );
        assert!(validate_data_consistency(&excluded).is_err());

        let empty = Ontology::new()
            .with_axiom(Axiom::DataPropertyOneOf {
                property: term("status"),
                values: Vec::new(),
            })
            .with_axiom(fact("Active"));
        assert!(validate_data_consistency(&empty).is_err());
    }

    #[test]
    fn subproperty_constraints_and_entailments_fail_closed() {
        fn subproperty() -> Axiom {
            Axiom::SubDataPropertyOf {
                sub: term("employee-id"),
                sup: term("person-id"),
            }
        }

        // An inherited range applies to values asserted through the child.
        let range = Ontology::from_iter([
            subproperty(),
            Axiom::DataPropertyRange {
                property: term("person-id"),
                datatype: Datatype::Integer,
            },
            fact("alice", "employee-id", Literal::String("bad".into())),
        ]);
        assert!(validate_data_consistency(&range).is_err());

        // A functional parent makes conflicting child assertions inconsistent.
        let functional = Ontology::from_iter([
            subproperty(),
            Axiom::FunctionalDataProperty(term("person-id")),
            fact("alice", "employee-id", Literal::Integer(1)),
            fact("alice", "employee-id", Literal::Integer(2)),
        ]);
        assert!(validate_data_consistency(&functional).is_err());

        // A negative parent assertion contradicts an entailed child assertion.
        let negative_parent = Ontology::from_iter([
            subproperty(),
            fact("alice", "employee-id", Literal::Integer(30)),
            negative("alice", "person-id", Literal::Integer(30)),
        ]);
        assert!(validate_data_consistency(&negative_parent).is_err());

        // Disjoint parents are reached through distinct child properties.
        let disjoint_parents = Ontology::from_iter([
            Axiom::SubDataPropertyOf {
                sub: term("work-contact"),
                sup: term("contact-left"),
            },
            Axiom::SubDataPropertyOf {
                sub: term("home-contact"),
                sup: term("contact-right"),
            },
            Axiom::DisjointDataProperties(vec![term("contact-left"), term("contact-right")]),
            fact("alice", "work-contact", Literal::String("a".into())),
            fact("alice", "home-contact", Literal::String("b".into())),
        ]);
        assert!(validate_data_consistency(&disjoint_parents).is_err());
    }

    #[test]
    fn data_property_domains_are_materialized_through_subproperties() {
        let ontology = Ontology::from_iter([
            Axiom::SubDataPropertyOf {
                sub: term("employee-id"),
                sup: term("person-id"),
            },
            Axiom::DataPropertyDomain {
                property: term("person-id"),
                class: ClassExpression::class(term("Person")),
            },
            Axiom::SameIndividuals(vec![term("alice"), term("bob")]),
            fact("alice", "employee-id", Literal::Integer(1)),
        ]);
        let materialized = materialize_data_domain_assertions(&ontology);
        assert!(materialized.axioms().iter().any(|axiom| matches!(
            axiom,
            Axiom::ClassAssertion {
                class: ClassExpression::Class(class),
                individual,
            } if *class == term("Person") && *individual == term("alice")
        )));
    }
}
