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
use chrono::{DateTime, Datelike, NaiveDate, Utc};
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

#[derive(Debug)]
struct DataConsistencyCacheEntry {
    fingerprint: u64,
    ontology: Arc<Ontology>,
    prefix_hash: Option<u64>,
    state: Arc<DataConsistencyState>,
    result: Result<()>,
}

/// Immutable counters for the process-local consistency cache.  They make the
/// storage governance path observable without weakening cache correctness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DataConsistencyCacheStats {
    pub exact_hits: u64,
    pub prefix_hits: u64,
    pub rebuilds: u64,
    pub delta_axioms: u64,
}

static EXACT_HITS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static PREFIX_HITS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static REBUILDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static DELTA_AXIOMS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Return the lifetime totals recorded by [`validate_data_consistency`].
pub fn data_consistency_cache_stats() -> DataConsistencyCacheStats {
    DataConsistencyCacheStats {
        exact_hits: EXACT_HITS.load(std::sync::atomic::Ordering::Relaxed),
        prefix_hits: PREFIX_HITS.load(std::sync::atomic::Ordering::Relaxed),
        rebuilds: REBUILDS.load(std::sync::atomic::Ordering::Relaxed),
        delta_axioms: DELTA_AXIOMS.load(std::sync::atomic::Ordering::Relaxed),
    }
}

fn record_exact_hit() {
    EXACT_HITS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    #[cfg(feature = "test-util")]
    THREAD_EXACT_HITS.with(|count| count.set(count.get() + 1));
}

fn record_prefix_hit() {
    PREFIX_HITS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    #[cfg(feature = "test-util")]
    THREAD_PREFIX_HITS.with(|count| count.set(count.get() + 1));
}

fn record_rebuild() {
    REBUILDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    #[cfg(feature = "test-util")]
    THREAD_REBUILDS.with(|count| count.set(count.get() + 1));
}

fn record_delta_axiom() {
    DELTA_AXIOMS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    #[cfg(feature = "test-util")]
    THREAD_DELTA_AXIOMS.with(|count| count.set(count.get() + 1));
}

// Per-thread counters let storage regressions assert cache behavior without
// observing unrelated traffic from concurrent tests.
#[cfg(feature = "test-util")]
thread_local! {
    static THREAD_EXACT_HITS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static THREAD_PREFIX_HITS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static THREAD_REBUILDS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static THREAD_DELTA_AXIOMS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(feature = "test-util")]
pub fn reset_data_consistency_thread_stats() {
    THREAD_EXACT_HITS.with(|count| count.set(0));
    THREAD_PREFIX_HITS.with(|count| count.set(0));
    THREAD_REBUILDS.with(|count| count.set(0));
    THREAD_DELTA_AXIOMS.with(|count| count.set(0));
}

#[cfg(feature = "test-util")]
pub fn data_consistency_thread_stats() -> DataConsistencyCacheStats {
    let exact_hits = THREAD_EXACT_HITS.with(std::cell::Cell::get);
    let prefix_hits = THREAD_PREFIX_HITS.with(std::cell::Cell::get);
    let rebuilds = THREAD_REBUILDS.with(std::cell::Cell::get);
    let delta_axioms = THREAD_DELTA_AXIOMS.with(std::cell::Cell::get);
    DataConsistencyCacheStats {
        exact_hits,
        prefix_hits,
        rebuilds,
        delta_axioms,
    }
}

type DataRestrictionIndex = BTreeMap<Term, Vec<(usize, Datatype, Vec<FacetValue>, Option<Regex>)>>;

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

    fn get(
        &mut self,
        key: u64,
        ontology: &Ontology,
    ) -> Option<(Result<()>, Arc<DataConsistencyState>)> {
        let position = self
            .entries
            .iter()
            .position(|entry| entry.fingerprint == key && entry.ontology.as_ref() == ontology)?;
        let entry = self.entries.remove(position);
        let entry = entry.expect("position came from entries");
        let result = entry.result.clone();
        let state = Arc::clone(&entry.state);
        self.entries.push_back(entry);
        Some((result, state))
    }

    #[cfg(test)]
    fn contains_key(&self, key: u64, ontology: &Ontology) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.fingerprint == key && entry.ontology.as_ref() == ontology)
    }

    fn put(
        &mut self,
        key: u64,
        ontology: &Ontology,
        state: DataConsistencyState,
        result: Result<()>,
    ) {
        self.entries.push_back(DataConsistencyCacheEntry {
            fingerprint: key,
            ontology: Arc::new(ontology.clone()),
            prefix_hash: data_projection_prefix_hashes(&data_consistency_projection(ontology))
                .pop(),
            state: Arc::new(state),
            result,
        });
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
        }
    }

    /// Restore an entry removed by [`find_prefix`], promoting it like a hit.
    fn restore(&mut self, entry: DataConsistencyCacheEntry) {
        self.entries.push_back(entry);
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
        }
    }

    /// Find the longest cached data-fragment projection that is a prefix of
    /// `query`.  Equality is axiom-by-axiom, so fingerprints never authorize a
    /// semantic hit.  The found entry is promoted for normal LRU behavior.
    fn find_prefix(&mut self, query: &[&Axiom]) -> Option<(usize, DataConsistencyCacheEntry)> {
        let query_hashes = data_projection_prefix_hashes(query);
        for length in (1..=query.len()).rev() {
            let hash = query_hashes[length - 1];
            for (position, entry) in self.entries.iter().enumerate() {
                if entry.prefix_hash != Some(hash)
                    || !projection_prefix_equals(entry.ontology.as_ref(), &query[..length])
                {
                    continue;
                }
                let entry = self.entries.remove(position)?;
                return Some((length, entry));
            }
        }
        None
    }

    /// Find a cached snapshot with the same data schema whose ABox differs by
    /// positive/negative assertion multiset operations.  Unlike prefix reuse,
    /// this supports replacement and deletion without treating an absent axiom
    /// as an OWL negative assertion.
    fn find_patch_base(
        &mut self,
        ontology: &Ontology,
    ) -> Option<(DataConsistencyCacheEntry, AssertionPatch)> {
        for position in (0..self.entries.len()).rev() {
            let Some(entry) = self.entries.get(position) else {
                continue;
            };
            if !entry.result.is_ok() || !same_data_schema(entry.ontology.as_ref(), ontology) {
                continue;
            }
            let Some(patch) = assertion_delta(entry.ontology.as_ref(), ontology) else {
                continue;
            };
            let entry = self.entries.remove(position)?;
            return Some((entry, patch));
        }
        None
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

/// A small process-local rolling hasher for candidate lookup.  Every candidate
/// is still verified axiom-by-axiom, so collisions only cost time, never soundness.
#[derive(Default)]
struct PrefixHasher(u64);

impl Hasher for PrefixHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3);
        }
    }
}

fn data_projection_prefix_hashes(projection: &[&Axiom]) -> Vec<u64> {
    let mut hasher = PrefixHasher::default();
    projection
        .iter()
        .map(|axiom| {
            axiom.hash(&mut hasher);
            hasher.finish()
        })
        .collect()
}

fn projection_prefix_equals(ontology: &Ontology, query: &[&Axiom]) -> bool {
    let candidate = ontology
        .axioms()
        .iter()
        .filter(|axiom| data_consistency_relevant(axiom))
        .collect::<Vec<_>>();
    candidate.len() == query.len()
        && candidate
            .into_iter()
            .zip(query)
            .all(|(left, right)| *left == **right)
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

/// Check asserted cardinality conflicts that the selected mature reasoner does
/// not yet cover completely.
///
/// This is an open-world check: absent values never violate a minimum, and
/// named individuals are merged only when `SameIndividuals` forces them
/// together. An upper-bound clash requires necessarily distinct witnesses, so
/// every rejection is a real OWL inconsistency even though the scan is
/// deliberately incomplete for inferred types and complex fillers.
pub(crate) fn validate_asserted_cardinality_consistency(ontology: &Ontology) -> Result<()> {
    let mut aliases = BTreeMap::new();
    let mut class_edges = BTreeMap::<Term, BTreeSet<Term>>::new();
    let mut object_bounds = BTreeMap::<(Term, Term, Term), (u32, u32)>::new();
    let mut data_bounds = BTreeMap::<(Term, Term, Datatype), (u32, u32)>::new();

    let intersect_bound = |bounds: &mut (u32, u32), bound: (u32, u32)| {
        bounds.0 = bounds.0.max(bound.0);
        bounds.1 = bounds.1.min(bound.1);
    };

    for axiom in ontology.axioms() {
        match axiom {
            Axiom::SameIndividuals(terms) if terms.len() >= 2 => {
                for pair in terms.windows(2) {
                    union(&mut aliases, &pair[0], &pair[1]);
                }
            }
            Axiom::SubClassOf { sub, sup } => {
                let ClassExpression::Class(source) = sub else {
                    continue;
                };
                match sup {
                    ClassExpression::Class(parent) => {
                        class_edges
                            .entry(source.clone())
                            .or_default()
                            .insert(parent.clone());
                    }
                    ClassExpression::ObjectMaxCardinality {
                        cardinality,
                        property,
                        value,
                    }
                    | ClassExpression::ObjectExactCardinality {
                        cardinality,
                        property,
                        value,
                    } => {
                        let ClassExpression::Class(target) = value.as_ref() else {
                            continue;
                        };
                        intersect_bound(
                            object_bounds
                                .entry((source.clone(), property.clone(), target.clone()))
                                .or_insert((0, u32::MAX)),
                            (0, *cardinality),
                        );
                    }
                    ClassExpression::DataMaxCardinality {
                        cardinality,
                        property,
                        range: DataRange::Datatype(datatype),
                    }
                    | ClassExpression::DataExactCardinality {
                        cardinality,
                        property,
                        range: DataRange::Datatype(datatype),
                    } => intersect_bound(
                        data_bounds
                            .entry((source.clone(), property.clone(), *datatype))
                            .or_insert((0, u32::MAX)),
                        (0, *cardinality),
                    ),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    let mut types_by_individual = BTreeMap::<Term, BTreeSet<Term>>::new();
    for axiom in ontology.axioms() {
        let Axiom::ClassAssertion { class, individual } = axiom else {
            continue;
        };
        let ClassExpression::Class(source) = class else {
            continue;
        };
        types_by_individual
            .entry(canonical_alias(&aliases, individual))
            .or_default()
            .insert(source.clone());
    }
    for types in types_by_individual.values_mut() {
        let direct = types.iter().cloned().collect::<Vec<_>>();
        for source in direct {
            let mut pending = vec![source];
            while let Some(current) = pending.pop() {
                for parent in class_edges.get(&current).into_iter().flatten() {
                    if types.insert(parent.clone()) {
                        pending.push(parent.clone());
                    }
                }
            }
        }
    }

    let mut object_facts = BTreeMap::<(Term, Term), BTreeSet<Term>>::new();
    let mut data_facts = BTreeMap::<(Term, Term), Vec<Literal>>::new();
    for axiom in ontology.axioms() {
        match axiom {
            Axiom::ObjectPropertyAssertion {
                property,
                subject,
                object,
            } => {
                let key = (canonical_alias(&aliases, subject), property.clone());
                object_facts.entry(key).or_default().insert(object.clone());
            }
            Axiom::DataPropertyAssertion {
                property,
                individual,
                value,
            } => {
                let key = (canonical_alias(&aliases, individual), property.clone());
                data_facts.entry(key).or_default().push(value.clone());
            }
            _ => {}
        }
    }

    for ((source, property, target), (_, max)) in object_bounds {
        for (individual, types) in &types_by_individual {
            if !types.contains(&source) {
                continue;
            }
            let witnesses = object_facts
                .get(&(individual.clone(), property.clone()))
                .into_iter()
                .flatten()
                .filter(|object| instance_of_named_class_term(object, &target, &class_edges))
                .map(|object| canonical_alias(&aliases, object))
                .collect::<BTreeSet<_>>();
            if witnesses.len() > max as usize {
                return Err(KnowledgeError::Invalid(format!(
                    "class `{source}` allows at most {max} `{property}` edges to `{target}`, \
                     but `{individual}` has {} distinct witnesses",
                    witnesses.len()
                )));
            }
        }
    }

    for ((source, property, datatype), (_, max)) in data_bounds {
        for (individual, types) in &types_by_individual {
            if !types.contains(&source) {
                continue;
            }
            let asserted = data_facts
                .get(&(individual.clone(), property.clone()))
                .map(Vec::as_slice)
                .unwrap_or_default();
            let mut distinct_values = Vec::new();
            for value in asserted {
                if value.datatype() != datatype {
                    continue;
                }
                if distinct_values
                    .iter()
                    .all(|existing| literals_are_necessarily_distinct(existing, value))
                {
                    distinct_values.push(value.clone());
                }
            }
            if distinct_values.len() > max as usize {
                return Err(KnowledgeError::Invalid(format!(
                    "class `{source}` allows at most {max} `{property}` values of type \
                     `{datatype:?}`, but `{individual}` has {} distinct values",
                    distinct_values.len()
                )));
            }
        }
    }

    Ok(())
}

fn canonical_alias(aliases: &BTreeMap<Term, Term>, term: &Term) -> Term {
    let mut current = term.clone();
    while let Some(next) = aliases.get(&current) {
        if *next == current {
            break;
        }
        current = next.clone();
    }
    current
}

fn instance_of_named_class_term(
    individual: &Term,
    declared: &Term,
    edges: &BTreeMap<Term, BTreeSet<Term>>,
) -> bool {
    let mut visited = BTreeSet::from([individual.clone()]);
    let mut pending = vec![individual.clone()];
    while let Some(current) = pending.pop() {
        if *declared == current {
            return true;
        }
        for parent in edges.get(&current).into_iter().flatten() {
            if visited.insert(parent.clone()) {
                pending.push(parent.clone());
            }
        }
    }
    false
}

fn literals_are_necessarily_distinct(left: &Literal, right: &Literal) -> bool {
    let ordering = compare_literals(left, right);
    match (left, right) {
        (Literal::Boolean(left), Literal::Boolean(right)) => left != right,
        (Literal::Integer(left), Literal::Integer(right)) => left != right,
        (Literal::String(left), Literal::String(right)) => left != right,
        (Literal::Double(_), Literal::Double(_)) => {
            ordering.is_some_and(|ordering| ordering != std::cmp::Ordering::Equal)
        }
        (Literal::Decimal(_), Literal::Decimal(_)) => {
            ordering.is_some_and(|ordering| ordering != std::cmp::Ordering::Equal)
        }
        (Literal::Date(_), Literal::Date(_)) => {
            ordering.is_some_and(|ordering| ordering != std::cmp::Ordering::Equal)
        }
        (Literal::DateTime(_), Literal::DateTime(_)) => {
            ordering.is_some_and(|ordering| ordering != std::cmp::Ordering::Equal)
        }
        _ => false,
    }
}
/// Reusable indexes for the deterministic asserted-data fragment.
///
/// A completed state is immutable in the cache.  Appending assertions and the
/// supported TBox constraints reuse that state; unsupported changes fall back
/// to a fresh full scan so a prefix hit can never hide a semantic delta.
#[derive(Debug, Clone, Default)]
struct DataConsistencyState {
    aliases: BTreeMap<Term, Term>,
    raw_positives: Vec<DataFact>,
    raw_negatives: Vec<DataFact>,
    functional: BTreeSet<Term>,
    disjoint: BTreeSet<(Term, Term)>,
    ranges: BTreeMap<Term, BTreeSet<Datatype>>,
    one_ofs: BTreeMap<Term, Vec<Vec<Literal>>>,
    restrictions: DataRestrictionIndex,
    range_expressions: BTreeMap<Term, Vec<DataRange>>,
    sub_data_properties: BTreeMap<Term, Term>,
    positives: Vec<DataFact>,
    negatives: Vec<DataFact>,
    positive_index: BTreeMap<(Term, Term), Vec<usize>>,
    negative_index: BTreeMap<(Term, Term), Vec<usize>>,
    properties_by_individual: BTreeMap<Term, BTreeSet<Term>>,
    functional_values: BTreeMap<(Term, Term), Literal>,
}

impl DataConsistencyState {
    fn from_ontology(ontology: &Ontology) -> Result<Self> {
        let mut state = Self::default();
        let mut raw_positives = Vec::new();
        let mut raw_negatives = Vec::new();
        let mut restriction_count = 0usize;

        for axiom in ontology.axioms() {
            match axiom {
                Axiom::SameIndividuals(terms) if terms.len() >= 2 => {
                    for pair in terms.windows(2) {
                        union(&mut state.aliases, &pair[0], &pair[1]);
                    }
                }
                Axiom::DataPropertyAssertion {
                    property,
                    individual,
                    value,
                } => raw_positives.push(DataFact {
                    individual: individual.clone(),
                    property: property.clone(),
                    value: value.clone(),
                }),
                Axiom::NegativeDataPropertyAssertion {
                    property,
                    individual,
                    value,
                } => raw_negatives.push(DataFact {
                    individual: individual.clone(),
                    property: property.clone(),
                    value: value.clone(),
                }),
                Axiom::FunctionalDataProperty(property) => {
                    state.functional.insert(property.clone());
                }
                Axiom::DisjointDataProperties(properties) if properties.len() >= 2 => {
                    for left in 0..properties.len() {
                        for right in left + 1..properties.len() {
                            let first = &properties[left];
                            let second = &properties[right];
                            state
                                .disjoint
                                .insert((first.min(second).clone(), first.max(second).clone()));
                        }
                    }
                }
                Axiom::DataPropertyRange { property, datatype } => {
                    state
                        .ranges
                        .entry(property.clone())
                        .or_default()
                        .insert(*datatype);
                }
                Axiom::DataPropertyOneOf { property, values } => {
                    state
                        .one_ofs
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
                    state
                        .restrictions
                        .entry(property.clone())
                        .or_default()
                        .push((restriction_count, *datatype, facets.clone(), pattern));
                    restriction_count += 1;
                }
                Axiom::DataPropertyDataRange { property, range } => {
                    validate_data_range(range, property)?;
                    state
                        .range_expressions
                        .entry(property.clone())
                        .or_default()
                        .push(range.clone());
                }
                Axiom::SubDataPropertyOf { sub, sup } => {
                    state.sub_data_properties.insert(sub.clone(), sup.clone());
                }
                _ => {}
            }
        }

        // Two different concrete ranges make every assertion violate at least
        // one of them under the current basic-datatype vocabulary.
        for (property, datatypes) in &state.ranges {
            if datatypes.len() > 1 {
                return Err(KnowledgeError::Invalid(format!(
                    "data property `{property}` has contradictory ranges: {datatypes:?}"
                )));
            }
        }

        // Materialize entailments through the data-property hierarchy before the
        // deterministic checks below.  Both polarities propagate upward: if
        // `work-email ⊑ email`, then positive/negative facts about `work-email`
        // entail the same polarity about `email`.
        state.raw_negatives = raw_negatives;
        state.raw_positives = raw_positives;
        state.negatives = state.expand_all(&state.raw_negatives);
        state.positives = state.expand_all(&state.raw_positives);

        // Negatives are indexed first so the first positive insertion observes
        // exactly the same closed snapshot as the previous full validator.
        for fact in std::mem::take(&mut state.negatives) {
            state.insert_negative(fact)?;
        }
        for fact in std::mem::take(&mut state.positives) {
            state.insert_positive(fact)?;
        }
        Ok(state)
    }

    fn expand_one(&self, fact: &DataFact, property: Term) -> DataFact {
        DataFact {
            individual: fact.individual.clone(),
            property,
            value: fact.value.clone(),
        }
    }

    fn expand_all(&self, facts: &[DataFact]) -> Vec<DataFact> {
        facts
            .iter()
            .flat_map(|fact| {
                property_ancestors(&self.sub_data_properties, &fact.property)
                    .into_iter()
                    .map(move |property| self.expand_one(fact, property))
            })
            .collect()
    }

    /// Re-expand retained assertions after an appended sub-property edge.
    /// Raw facts are kept separately because the indexed positives/negatives
    /// contain prior hierarchy entailments and must not be expanded twice.
    fn rebuild_expanded_assertions(&mut self) -> Result<()> {
        let negatives = self.expand_all(&self.raw_negatives);
        let positives = self.expand_all(&self.raw_positives);
        self.negatives = negatives;
        self.positives = positives;
        self.positive_index.clear();
        self.negative_index.clear();
        self.properties_by_individual.clear();
        self.functional_values.clear();

        for fact in std::mem::take(&mut self.negatives) {
            self.insert_negative(fact)?;
        }
        for fact in std::mem::take(&mut self.positives) {
            self.insert_positive(fact)?;
        }
        Ok(())
    }

    fn apply_subproperty_delta(&mut self, sub: &Term, sup: &Term) -> Result<()> {
        self.sub_data_properties.insert(sub.clone(), sup.clone());
        self.rebuild_expanded_assertions()
    }

    /// Identify the raw assertions whose expansion intersects a removal patch.
    /// The exact expanded multiset must agree before either raw list is edited,
    /// so a stale hierarchy mapping can only cause a safe full rebuild.
    fn matching_raw_assertions(
        &self,
        raw: &[DataFact],
        removed: &[DataFact],
    ) -> Option<Vec<DataFact>> {
        if removed.is_empty() {
            return Some(Vec::new());
        }
        let selected = raw
            .iter()
            .filter(|fact| {
                self.expand_all(std::slice::from_ref(fact))
                    .iter()
                    .any(|expanded| removed.contains(expanded))
            })
            .cloned()
            .collect::<Vec<_>>();
        let expected = self.expand_all(&selected);
        let mut expected_keys = expected
            .iter()
            .map(|fact| assertion_key(AssertionPolarity::Positive, fact))
            .collect::<Vec<_>>();
        let mut removed_keys = removed
            .iter()
            .map(|fact| assertion_key(AssertionPolarity::Positive, fact))
            .collect::<Vec<_>>();
        expected_keys.sort();
        removed_keys.sort();
        (expected_keys == removed_keys).then_some(selected)
    }

    fn forget_removed_raw_assertions(&mut self, patch: &AssertionPatch) -> Option<()> {
        let positive_removals = self.expand_all(&patch.remove_positive);
        let negative_removals = self.expand_all(&patch.remove_negative);
        let positives = self.matching_raw_assertions(&self.raw_positives, &positive_removals)?;
        let negatives = self.matching_raw_assertions(&self.raw_negatives, &negative_removals)?;
        for fact in positives {
            let position = self.raw_positives.iter().position(|raw| raw == &fact)?;
            self.raw_positives.swap_remove(position);
        }
        for fact in negatives {
            let position = self.raw_negatives.iter().position(|raw| raw == &fact)?;
            self.raw_negatives.swap_remove(position);
        }
        Some(())
    }

    fn insert_added_positive(&mut self, fact: DataFact) -> Result<()> {
        self.raw_positives.push(fact.clone());
        for expanded in self.expand_all(std::slice::from_ref(&fact)) {
            self.insert_positive(expanded)?;
        }
        Ok(())
    }

    fn insert_added_negative(&mut self, fact: DataFact) -> Result<()> {
        self.raw_negatives.push(fact.clone());
        for expanded in self.expand_all(std::slice::from_ref(&fact)) {
            self.insert_negative(expanded)?;
        }
        Ok(())
    }

    fn canonical_individual(&self, individual: &Term) -> Term {
        let mut current = individual.clone();
        while let Some(next) = self.aliases.get(&current) {
            if *next == current {
                break;
            }
            current = next.clone();
        }
        current
    }

    /// Check every schema-derived constraint attached to a positive fact.
    /// Identity lookup and index mutation stay with the caller so the same
    /// logic can revalidate retained facts after an appended TBox constraint.
    fn validate_positive_constraints(&self, fact: &DataFact, individual: &Term) -> Result<()> {
        if let Some(candidate_ranges) = self.range_expressions.get(&fact.property) {
            for range in candidate_ranges {
                if !data_range_admits(range, &fact.value) {
                    return Err(KnowledgeError::Invalid(format!(
                        "data property `{}` range rejects `{individual}` value {:?}",
                        fact.property, fact.value
                    )));
                }
            }
        }

        if self
            .ranges
            .get(&fact.property)
            .is_some_and(|allowed| !allowed.contains(&fact.value.datatype()))
        {
            return Err(KnowledgeError::Invalid(format!(
                "data property `{}` range rejects `{individual}` value {:?}",
                fact.property, fact.value
            )));
        }

        if let Some(matching) = self.restrictions.get(&fact.property) {
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

        if let Some(enumerations) = self.one_ofs.get(&fact.property) {
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

        if let Some(properties) = self.properties_by_individual.get(individual) {
            for existing in properties {
                if *existing == fact.property {
                    continue;
                }
                let pair = if existing <= &fact.property {
                    (existing.clone(), fact.property.clone())
                } else {
                    (fact.property.clone(), existing.clone())
                };
                if self.disjoint.contains(&pair) {
                    return Err(KnowledgeError::Invalid(format!(
                        "data properties `{}` and `{}` are disjoint but both are asserted for `{individual}`",
                        pair.0, pair.1
                    )));
                }
            }
        }

        if self.functional.contains(&fact.property)
            && let Some(existing) = self
                .functional_values
                .get(&(individual.clone(), fact.property.clone()))
            && !literal_equal(existing, &fact.value)
        {
            return Err(KnowledgeError::Invalid(format!(
                "functional data property `{}` has conflicting values for `{individual}`",
                fact.property
            )));
        }

        Ok(())
    }

    fn revalidate_positive_constraints(&self) -> Result<()> {
        for fact in &self.positives {
            let individual = self.canonical_individual(&fact.individual);
            validate_literal(&fact.property, &fact.value)?;
            self.validate_positive_constraints(fact, &individual)?;
        }
        Ok(())
    }

    /// Retroactively make a property functional.  Unlike ordinary insertion,
    /// the retained facts cannot rely on `functional_values`: an earlier
    /// non-functional snapshot never populated that bucket.
    fn enforce_functional_property(&mut self, property: &Term) -> Result<()> {
        self.functional.insert(property.clone());
        let mut values = BTreeMap::new();
        for fact in &self.positives {
            if fact.property != *property {
                continue;
            }
            let key = (
                self.canonical_individual(&fact.individual),
                property.clone(),
            );
            match values.get(&key) {
                Some(existing) if literal_equal(existing, &fact.value) => {}
                Some(_) => {
                    return Err(KnowledgeError::Invalid(format!(
                        "functional data property `{}` has conflicting values for `{}`",
                        property, key.0
                    )));
                }
                None => {
                    values.insert(key, fact.value.clone());
                }
            }
        }
        self.functional_values
            .retain(|(_, existing_property), _| existing_property != property);
        self.functional_values.extend(values);
        Ok(())
    }

    fn insert_positive(&mut self, fact: DataFact) -> Result<()> {
        validate_literal(&fact.property, &fact.value)?;
        let individual = self.canonical_individual(&fact.individual);
        self.validate_positive_constraints(&fact, &individual)?;

        let contradicted = self
            .negative_index
            .get(&(individual.clone(), fact.property.clone()))
            .is_some_and(|candidates| {
                candidates.iter().any(|index| {
                    self.negatives
                        .get(*index)
                        .is_some_and(|negative| literal_equal(&negative.value, &fact.value))
                })
            });
        if contradicted {
            return Err(KnowledgeError::Invalid(format!(
                "individual `{individual}` has positive and negative assertions for data property `{}`",
                fact.property
            )));
        }

        let properties = self
            .properties_by_individual
            .entry(individual.clone())
            .or_default();
        properties.insert(fact.property.clone());

        if self.functional.contains(&fact.property) {
            let key = (individual.clone(), fact.property.clone());
            self.functional_values
                .entry(key)
                .or_insert_with(|| fact.value.clone());
        }

        let key = (individual, fact.property.clone());
        let index = self.positives.len();
        self.positives.push(fact);
        self.positive_index.entry(key).or_default().push(index);
        Ok(())
    }

    fn insert_negative(&mut self, fact: DataFact) -> Result<()> {
        validate_literal(&fact.property, &fact.value)?;
        let individual = self.canonical_individual(&fact.individual);
        let contradicted = self
            .positive_index
            .get(&(individual.clone(), fact.property.clone()))
            .is_some_and(|candidates| {
                candidates.iter().any(|index| {
                    self.positives
                        .get(*index)
                        .is_some_and(|positive| literal_equal(&positive.value, &fact.value))
                })
            });
        if contradicted {
            return Err(KnowledgeError::Invalid(format!(
                "individual `{individual}` has positive and negative assertions for data property `{}`",
                fact.property
            )));
        }

        let key = (individual, fact.property.clone());
        let index = self.negatives.len();
        self.negatives.push(fact);
        self.negative_index.entry(key).or_default().push(index);
        Ok(())
    }

    fn positive_matches(&self, fact: &DataFact, index: usize) -> bool {
        self.positives.get(index).is_some_and(|stored| {
            stored.property == fact.property
                && literal_equal(&stored.value, &fact.value)
                && self.canonical_individual(&stored.individual) == fact.individual
        })
    }

    fn can_remove_positive(&self, fact: &DataFact) -> bool {
        let key = (
            self.canonical_individual(&fact.individual),
            fact.property.clone(),
        );
        self.positive_index.get(&key).is_some_and(|candidates| {
            candidates
                .iter()
                .any(|index| self.positive_matches(fact, *index))
        })
    }

    fn can_remove_negative(&self, fact: &DataFact) -> bool {
        let key = (
            self.canonical_individual(&fact.individual),
            fact.property.clone(),
        );
        self.negative_index.get(&key).is_some_and(|candidates| {
            candidates
                .iter()
                .any(|index| self.negative_matches(fact, *index))
        })
    }

    fn negative_matches(&self, fact: &DataFact, index: usize) -> bool {
        self.negatives.get(index).is_some_and(|stored| {
            stored.property == fact.property
                && literal_equal(&stored.value, &fact.value)
                && self.canonical_individual(&stored.individual) == fact.individual
        })
    }

    fn remove_positive(&mut self, fact: &DataFact) -> Option<()> {
        let individual = self.canonical_individual(&fact.individual);
        let key = (individual.clone(), fact.property.clone());
        let candidate = self.positive_index.get(&key).and_then(|candidates| {
            candidates.iter().copied().find(|index| {
                self.positives.get(*index).is_some_and(|stored| {
                    stored.property == fact.property
                        && literal_equal(&stored.value, &fact.value)
                        && self.canonical_individual(&stored.individual) == individual
                })
            })
        })?;
        let candidates = self.positive_index.get_mut(&key)?;
        let occurrence = candidates
            .iter()
            .position(|index| *index == candidate)
            .expect("candidate index remains indexed");
        let position = candidates.remove(occurrence);
        if candidates.is_empty() {
            self.positive_index.remove(&key);
        }

        self.positives.swap_remove(position);
        let last = self.positives.len();
        if position < last {
            let previous_position = last;
            let moved = self.positives[position].clone();
            let moved_individual = self.canonical_individual(&moved.individual);
            let moved_key = (moved_individual.clone(), moved.property.clone());
            let candidates = self.positive_index.get_mut(&moved_key)?;
            let moved_position = candidates
                .iter()
                .position(|index| *index == previous_position)?;
            candidates[moved_position] = position;
        }

        if !self.positive_index.contains_key(&key)
            && let Some(properties) = self.properties_by_individual.get_mut(&individual)
        {
            properties.remove(&fact.property);
            if properties.is_empty() {
                self.properties_by_individual.remove(&individual);
            }
        }
        if self.functional.contains(&fact.property)
            && self
                .functional_values
                .get(&key)
                .is_some_and(|value| literal_equal(value, &fact.value))
            && !self.positives.iter().any(|stored| {
                self.canonical_individual(&stored.individual) == individual
                    && stored.property == fact.property
                    && literal_equal(&stored.value, &fact.value)
            })
        {
            self.functional_values.remove(&key);
        }
        Some(())
    }

    fn remove_negative(&mut self, fact: &DataFact) -> Option<()> {
        let individual = self.canonical_individual(&fact.individual);
        let key = (individual.clone(), fact.property.clone());
        let candidate = self.negative_index.get(&key).and_then(|candidates| {
            candidates.iter().copied().find(|index| {
                self.negatives.get(*index).is_some_and(|stored| {
                    stored.property == fact.property
                        && literal_equal(&stored.value, &fact.value)
                        && self.canonical_individual(&stored.individual) == individual
                })
            })
        })?;
        let candidates = self.negative_index.get_mut(&key)?;
        let occurrence = candidates
            .iter()
            .position(|index| *index == candidate)
            .expect("candidate index remains indexed");
        let position = candidates.remove(occurrence);
        if candidates.is_empty() {
            self.negative_index.remove(&key);
        }

        self.negatives.swap_remove(position);
        let last = self.negatives.len();
        if position < last {
            let previous_position = last;
            let moved = self.negatives[position].clone();
            let moved_individual = self.canonical_individual(&moved.individual);
            let moved_key = (moved_individual.clone(), moved.property.clone());
            let candidates = self.negative_index.get_mut(&moved_key)?;
            let moved_position = candidates
                .iter()
                .position(|index| *index == previous_position)?;
            candidates[moved_position] = position;
        }
        Some(())
    }

    fn merge_same_individuals(&mut self, terms: &[Term]) -> Result<()> {
        let mut identity_changed = false;
        for pair in terms.windows(2) {
            let left_before = self.canonical_individual(&pair[0]);
            let right_before = self.canonical_individual(&pair[1]);
            union(&mut self.aliases, &pair[0], &pair[1]);
            identity_changed |= left_before != self.canonical_individual(&pair[0])
                || right_before != self.canonical_individual(&pair[1]);
        }
        if !identity_changed {
            return Ok(());
        }

        // Existing assertions retain their original individual name, so changing
        // the identity mapping is sound only after every polarity is indexed
        // again under the merged canonical individual.
        let positives = std::mem::take(&mut self.positives);
        let negatives = std::mem::take(&mut self.negatives);
        self.positive_index.clear();
        self.negative_index.clear();
        self.properties_by_individual.clear();
        self.functional_values.clear();

        for fact in negatives {
            self.insert_negative(fact)?;
        }
        for fact in positives {
            self.insert_positive(fact)?;
        }
        Ok(())
    }

    /// Apply an appended constraint whose semantics affect only schema buckets
    /// and positive-fact admission.
    fn apply_tbox_delta(&mut self, axiom: &Axiom) -> Result<()> {
        match axiom {
            Axiom::DataPropertyDomain { .. } => Ok(()),
            Axiom::DataPropertyRange { property, datatype } => {
                let count = {
                    let allowed = self.ranges.entry(property.clone()).or_default();
                    allowed.insert(*datatype);
                    allowed.len()
                };
                if count > 1 {
                    let datatypes = &self.ranges[property];
                    return Err(KnowledgeError::Invalid(format!(
                        "data property `{property}` has contradictory ranges: {datatypes:?}"
                    )));
                }
                self.revalidate_positive_constraints()
            }
            Axiom::DataPropertyOneOf { property, values } => {
                self.one_ofs
                    .entry(property.clone())
                    .or_default()
                    .push(values.clone());
                self.revalidate_positive_constraints()
            }
            Axiom::DataPropertyRangeRestriction {
                property,
                datatype,
                facets,
            } => {
                let pattern = validate_restriction(property, datatype, facets)?;
                let restriction_index = self
                    .restrictions
                    .values()
                    .flat_map(|entries| entries.iter().map(|(index, _, _, _)| *index))
                    .max()
                    .map_or(0, |index| index + 1);
                self.restrictions
                    .entry(property.clone())
                    .or_default()
                    .push((restriction_index, *datatype, facets.clone(), pattern));
                self.revalidate_positive_constraints()
            }
            Axiom::DataPropertyDataRange { property, range } => {
                validate_data_range(range, property)?;
                self.range_expressions
                    .entry(property.clone())
                    .or_default()
                    .push(range.clone());
                self.revalidate_positive_constraints()
            }
            Axiom::FunctionalDataProperty(property) => self.enforce_functional_property(property),
            Axiom::DisjointDataProperties(properties) if properties.len() >= 2 => {
                for left in 0..properties.len() {
                    for right in left + 1..properties.len() {
                        let first = &properties[left];
                        let second = &properties[right];
                        self.disjoint
                            .insert((first.min(second).clone(), first.max(second).clone()));
                    }
                }
                self.revalidate_positive_constraints()
            }
            Axiom::DisjointDataProperties(_) => Ok(()),
            _ => Err(KnowledgeError::Invalid(
                "TBox delta supports only appended data constraints".to_owned(),
            )),
        }
    }

    /// Apply an append-only ABox fact to an already indexed snapshot.
    ///
    /// A real SameIndividuals axiom changes the canonical identity of existing
    /// assertions, so its cached index is rebuilt around the merged identity.
    /// Inert one-term axioms remain non-incremental and fall back to the full
    /// scan used by cold-path benchmarks.
    fn insert_delta(&mut self, axiom: &Axiom) -> Result<()> {
        match axiom {
            Axiom::SameIndividuals(terms) if terms.len() >= 2 => self.merge_same_individuals(terms),
            Axiom::SubDataPropertyOf { sub, sup } => self.apply_subproperty_delta(sub, sup),
            Axiom::DataPropertyDomain { .. }
            | Axiom::DataPropertyRange { .. }
            | Axiom::DataPropertyOneOf { .. }
            | Axiom::DataPropertyRangeRestriction { .. }
            | Axiom::DataPropertyDataRange { .. }
            | Axiom::FunctionalDataProperty(_)
            | Axiom::DisjointDataProperties(_) => self.apply_tbox_delta(axiom),
            Axiom::DataPropertyAssertion {
                property,
                individual,
                value,
            } => self.insert_added_positive(DataFact {
                individual: individual.clone(),
                property: property.clone(),
                value: value.clone(),
            }),
            Axiom::NegativeDataPropertyAssertion {
                property,
                individual,
                value,
            } => self.insert_added_negative(DataFact {
                individual: individual.clone(),
                property: property.clone(),
                value: value.clone(),
            }),
            _ => Err(KnowledgeError::Invalid(
                "data consistency delta supports only appended data-property assertions".to_owned(),
            )),
        }
    }
}

fn data_consistency_relevant(axiom: &Axiom) -> bool {
    matches!(
        axiom,
        Axiom::SameIndividuals(_)
            | Axiom::DataPropertyAssertion { .. }
            | Axiom::NegativeDataPropertyAssertion { .. }
    ) || is_incremental_tbox_axiom(axiom)
}

fn is_incremental_data_assertion(axiom: &Axiom) -> bool {
    matches!(
        axiom,
        Axiom::DataPropertyAssertion { .. } | Axiom::NegativeDataPropertyAssertion { .. }
    )
}

fn is_incremental_identity_axiom(axiom: &Axiom) -> bool {
    matches!(axiom, Axiom::SameIndividuals(terms) if terms.len() >= 2)
}

fn is_incremental_tbox_axiom(axiom: &Axiom) -> bool {
    matches!(
        axiom,
        Axiom::DataPropertyDomain { .. }
            | Axiom::DataPropertyRange { .. }
            | Axiom::DataPropertyOneOf { .. }
            | Axiom::DataPropertyRangeRestriction { .. }
            | Axiom::DataPropertyDataRange { .. }
            | Axiom::FunctionalDataProperty(_)
            | Axiom::DisjointDataProperties(_)
            | Axiom::SubDataPropertyOf { .. }
    )
}

/// One decision governs both the validator branch and its regression so the
/// accepted append-only vocabulary cannot silently diverge between the two.
fn supports_incremental_suffix(suffix: &[&Axiom]) -> bool {
    suffix.iter().all(|axiom| {
        is_incremental_data_assertion(axiom)
            || is_incremental_identity_axiom(axiom)
            || is_incremental_tbox_axiom(axiom)
    })
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum AssertionPolarity {
    Positive,
    Negative,
}

#[derive(Debug, Default)]
struct AssertionPatch {
    remove_positive: Vec<DataFact>,
    add_positive: Vec<DataFact>,
    remove_negative: Vec<DataFact>,
    add_negative: Vec<DataFact>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct AssertionKey {
    polarity: AssertionPolarity,
    individual: Term,
    property: Term,
    literal: String,
}

fn assertion_fact(axiom: &Axiom) -> Option<(AssertionPolarity, DataFact)> {
    let (polarity, property, individual, value) = match axiom {
        Axiom::DataPropertyAssertion {
            property,
            individual,
            value,
        } => Some((AssertionPolarity::Positive, property, individual, value)),
        Axiom::NegativeDataPropertyAssertion {
            property,
            individual,
            value,
        } => Some((AssertionPolarity::Negative, property, individual, value)),
        _ => None,
    }?;
    Some((
        polarity,
        DataFact {
            individual: individual.clone(),
            property: property.clone(),
            value: value.clone(),
        },
    ))
}

fn literal_cache_key(value: &Literal) -> String {
    let datatype = value.datatype();
    let lexical = match value {
        Literal::Decimal(value) => canonical_decimal(value).unwrap_or_else(|| value.clone()),
        Literal::Date(value) => date_value(value)
            .map(|date| date.num_days_from_ce().to_string())
            .unwrap_or_else(|| value.clone()),
        Literal::DateTime(value) => date_time_value(value)
            .map(|instant| {
                format!(
                    "{}:{:09}",
                    instant.timestamp(),
                    instant.timestamp_subsec_nanos()
                )
            })
            .unwrap_or_else(|| value.clone()),
        Literal::Double(value) => {
            if *value == 0.0 {
                "0".to_owned()
            } else {
                value.to_bits().to_string()
            }
        }
        Literal::Boolean(value) => value.to_string(),
        Literal::Integer(value) => value.to_string(),
        Literal::String(value) => value.clone(),
    };
    format!("{datatype:?}:{lexical}")
}

fn assertion_key(polarity: AssertionPolarity, fact: &DataFact) -> AssertionKey {
    AssertionKey {
        polarity,
        individual: fact.individual.clone(),
        property: fact.property.clone(),
        literal: literal_cache_key(&fact.value),
    }
}

/// Compare the fixed constraint/identity part of two data projections.
///
/// Assertions are deliberately excluded; they are handled as multiset deltas.
/// Requiring the retained schema to be exactly equal keeps SameIndividuals,
/// subproperties, ranges and restrictions outside the reach of patch reuse.
fn same_data_schema(left: &Ontology, right: &Ontology) -> bool {
    let left = left
        .axioms()
        .iter()
        .filter(|axiom| data_consistency_relevant(axiom) && !is_incremental_data_assertion(axiom))
        .collect::<Vec<_>>();
    let right = right
        .axioms()
        .iter()
        .filter(|axiom| data_consistency_relevant(axiom) && !is_incremental_data_assertion(axiom))
        .collect::<Vec<_>>();
    left.len() == right.len()
        && left
            .into_iter()
            .zip(right)
            .all(|(left, right)| left == right)
}

/// Return the symmetric difference between two asserted ABox multisets.
/// Net-equal duplicates cancel, while repeated distinct occurrences retain the
/// expected multiplicity.
fn assertion_delta(base: &Ontology, next: &Ontology) -> Option<AssertionPatch> {
    let mut counts = BTreeMap::new();
    for axiom in base
        .axioms()
        .iter()
        .filter(|axiom| is_incremental_data_assertion(axiom))
    {
        let (polarity, fact) = assertion_fact(axiom)?;
        let entry =
            counts
                .entry(assertion_key(polarity, &fact))
                .or_insert((0i64, polarity, fact.clone()));
        entry.0 -= 1;
        entry.2 = fact;
    }
    for axiom in next
        .axioms()
        .iter()
        .filter(|axiom| is_incremental_data_assertion(axiom))
    {
        let (polarity, fact) = assertion_fact(axiom)?;
        let entry =
            counts
                .entry(assertion_key(polarity, &fact))
                .or_insert((0i64, polarity, fact.clone()));
        entry.0 += 1;
        entry.2 = fact;
    }

    let mut patch = AssertionPatch::default();
    for (_, (count, polarity, fact)) in counts {
        match (polarity, count) {
            (AssertionPolarity::Positive, count) if count < 0 => patch
                .remove_positive
                .extend(std::iter::repeat_n(fact, (-count) as usize)),
            (AssertionPolarity::Positive, count) if count > 0 => patch
                .add_positive
                .extend(std::iter::repeat_n(fact, count as usize)),
            (AssertionPolarity::Negative, count) if count < 0 => patch
                .remove_negative
                .extend(std::iter::repeat_n(fact, (-count) as usize)),
            (AssertionPolarity::Negative, count) if count > 0 => patch
                .add_negative
                .extend(std::iter::repeat_n(fact, count as usize)),
            _ => {}
        }
    }
    Some(patch)
}

fn data_consistency_projection(ontology: &Ontology) -> Vec<&Axiom> {
    ontology
        .axioms()
        .iter()
        .filter(|axiom| data_consistency_relevant(axiom))
        .collect()
}

/// Validate the asserted data fragment that is not yet covered soundly by the
/// external tableau.  The checks are conservative for the supported vocabulary.
pub fn validate_data_consistency(ontology: &Ontology) -> Result<()> {
    let key = ontology_fingerprint(ontology);
    let projection = data_consistency_projection(ontology);
    let mut cache = data_consistency_cache()
        .lock()
        .expect("data consistency cache mutex poisoned");

    if let Some((result, _)) = cache.get(key, ontology) {
        record_exact_hit();
        return result;
    }

    // Ignore non-data axioms when locating a reusable prefix.  Live snapshots
    // commonly rewrite their trailing DifferentIndividuals list as objects are
    // added; that change does not affect this deterministic data fragment.
    if let Some((prefix_length, mut entry)) = cache.find_prefix(&projection) {
        record_prefix_hit();
        if prefix_length == projection.len() {
            let result = entry.result.clone();
            cache.restore(entry);
            return result;
        }
        let suffix = &projection[prefix_length..];
        if entry.result.is_ok() && supports_incremental_suffix(suffix) {
            // Reuse the retained index in place when this is its last cache
            // reference.  The fallback clone keeps the old prefix available and
            // preserves correctness under the unusual case of a shared state.
            let cached_state = entry.state;
            let (mut delta_state, restored_entry) = match Arc::try_unwrap(cached_state) {
                Ok(state) => (state, None),
                Err(state) => {
                    let cloned_state = (*state).clone();
                    entry.state = state;
                    (cloned_state, Some(entry))
                }
            };
            if let Some(restored_entry) = restored_entry {
                cache.restore(restored_entry);
            }
            let mut outcome = Ok(());
            for axiom in suffix {
                match delta_state.insert_delta(axiom) {
                    Ok(()) => {
                        record_delta_axiom();
                    }
                    Err(error) => {
                        outcome = Err(error);
                        break;
                    }
                }
            }
            cache.put(key, ontology, delta_state, outcome.clone());
            return outcome;
        }
    }

    // Replacement/deletion snapshots do not share a projection prefix with the
    // previous ABox.  Reuse their completed index only when the constraint and
    // identity schema is exactly unchanged and the difference reduces to data
    // assertion insert/remove operations.
    if let Some((mut entry, patch)) = cache.find_patch_base(ontology) {
        record_prefix_hit();
        let cached_state = entry.state;
        let (mut delta_state, restored_entry) = match Arc::try_unwrap(cached_state) {
            Ok(state) => (state, None),
            Err(state) => {
                let cloned_state = (*state).clone();
                entry.state = state;
                (cloned_state, Some(entry))
            }
        };
        if let Some(restored_entry) = restored_entry {
            cache.restore(restored_entry);
        }

        // Expand raw assertions through the unchanged subproperty hierarchy
        // and prove every removal exists before mutating the cloned index.
        let mut removals = Vec::new();
        for fact in &patch.remove_positive {
            removals.extend(
                delta_state
                    .expand_all(std::slice::from_ref(fact))
                    .into_iter()
                    .map(|fact| (false, fact)),
            );
        }
        for fact in &patch.remove_negative {
            removals.extend(
                delta_state
                    .expand_all(std::slice::from_ref(fact))
                    .into_iter()
                    .map(|fact| (true, fact)),
            );
        }
        let removable = removals.iter().all(|(negative, fact)| {
            if *negative {
                delta_state.can_remove_negative(fact)
            } else {
                delta_state.can_remove_positive(fact)
            }
        });

        if removable && delta_state.forget_removed_raw_assertions(&patch).is_some() {
            let mut outcome = Ok(());
            for (negative, fact) in removals {
                let removed = if negative {
                    delta_state.remove_negative(&fact)
                } else {
                    delta_state.remove_positive(&fact)
                };
                if removed.is_some() {
                    record_delta_axiom();
                }
            }
            for fact in patch.add_positive {
                if let Err(error) = delta_state.insert_added_positive(fact) {
                    outcome = Err(error);
                    break;
                }
                record_delta_axiom();
            }
            if outcome.is_ok() {
                for fact in patch.add_negative {
                    if let Err(error) = delta_state.insert_added_negative(fact) {
                        outcome = Err(error);
                        break;
                    }
                    record_delta_axiom();
                }
            }
            cache.put(key, ontology, delta_state, outcome.clone());
            return outcome;
        }
    }

    let (state, result) = match DataConsistencyState::from_ontology(ontology) {
        Ok(state) => (state, Ok(())),
        Err(error) => {
            let state = DataConsistencyState::default();
            (state, Err(error))
        }
    };
    record_rebuild();
    cache.put(key, ontology, state, result.clone());
    result
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
                .map(|(result, _)| result)
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
                .is_some_and(|(result, _)| result.is_err())
        );
    }

    #[test]
    fn concurrent_validation_preserves_distinct_results_and_errors() {
        const WORKERS: usize = 8;
        const ROUNDS: usize = 128;

        let results = std::thread::scope(|scope| {
            (0..WORKERS)
                .map(|worker| {
                    scope.spawn(move || {
                        let individual = Term::new(format!("concurrent-{worker}"));
                        let valid = Ontology::new()
                            .with_axiom(Axiom::FunctionalDataProperty(term("age")))
                            .with_axiom(fact(individual.as_str(), "age", Literal::Integer(30)));
                        let conflicting = valid.clone().with_axiom(fact(
                            individual.as_str(),
                            "age",
                            Literal::Integer(31),
                        ));

                        for _ in 0..ROUNDS {
                            assert!(validate_data_consistency(&valid).is_ok());
                            assert!(validate_data_consistency(&conflicting).is_err());
                        }
                    })
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(std::thread::ScopedJoinHandle::join)
                .collect::<Vec<_>>()
        });

        for result in results {
            result.unwrap();
        }
    }

    #[test]
    fn concurrent_replacements_preserve_distinct_snapshots() {
        const WORKERS: usize = 4;
        const ROUNDS: usize = 48;

        let results = std::thread::scope(|scope| {
            (0..WORKERS)
                .map(|worker| {
                    scope.spawn(move || {
                        let schema = Ontology::new().with_axiom(Axiom::FunctionalDataProperty(
                            term("concurrent-replace-status"),
                        ));
                        let first = schema.clone().with_axiom(fact(
                            &format!("concurrent-replace-{worker}"),
                            "concurrent-replace-status",
                            Literal::Integer(worker as i64),
                        ));
                        let second = schema.clone().with_axiom(fact(
                            &format!("concurrent-replace-{worker}"),
                            "concurrent-replace-status",
                            Literal::Integer(worker as i64 + 100),
                        ));

                        for _ in 0..ROUNDS {
                            assert!(validate_data_consistency(&first).is_ok());
                            assert!(validate_data_consistency(&second).is_ok());
                        }
                    })
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(std::thread::ScopedJoinHandle::join)
                .collect::<Vec<_>>()
        });

        for result in results {
            result.unwrap();
        }
    }

    #[test]
    fn appended_abox_facts_reuse_the_cached_data_prefix() {
        let base = (0..32).fold(
            Ontology::new().with_axiom(Axiom::FunctionalDataProperty(term("status"))),
            |ontology, index| {
                ontology.with_axiom(fact(
                    &format!("delta-{index}"),
                    "status",
                    Literal::String("active".into()),
                ))
            },
        );
        assert!(validate_data_consistency(&base).is_ok());

        let extended = base.clone().with_axiom(fact(
            "delta-new",
            "status",
            Literal::String("active".into()),
        ));
        // Parallel tests share the global LRU and may evict our entry between
        // calls, so assert on observable correctness rather than probing the
        // cache directly.  Prefix-reuse mechanics are covered by the
        // single-threaded governance_cache integration test in oaip-storage.
        assert!(validate_data_consistency(&extended).is_ok());
    }

    #[test]
    fn appended_same_individuals_reindex_existing_assertions() {
        let schema =
            Ontology::new().with_axiom(Axiom::FunctionalDataProperty(term("same-functional")));
        let base = schema
            .clone()
            .with_axiom(fact(
                "same-a",
                "same-functional",
                Literal::String("first".into()),
            ))
            .with_axiom(fact(
                "same-b",
                "same-functional",
                Literal::String("second".into()),
            ));
        let same = Axiom::SameIndividuals(vec![term("same-a"), term("same-b")]);

        assert!(validate_data_consistency(&base).is_ok());
        let mut state = DataConsistencyState::from_ontology(&base).unwrap();
        assert!(is_incremental_identity_axiom(&same));
        assert!(state.insert_delta(&same).is_err());

        let extended = base.clone().with_axiom(same);
        let candidate = data_consistency_cache()
            .lock()
            .expect("cache mutex")
            .find_prefix(&data_consistency_projection(&extended));
        let Some((prefix_length, entry)) = candidate else {
            panic!("a real SameIndividuals suffix should use the append-only path");
        };
        assert_eq!(data_consistency_projection(&base).len(), prefix_length);
        assert_eq!(
            data_consistency_projection(&extended).len(),
            prefix_length + 1
        );
        data_consistency_cache()
            .lock()
            .expect("cache mutex")
            .restore(entry);
        assert!(validate_data_consistency(&extended).is_err());
    }

    #[test]
    fn appended_same_individuals_preserve_disjoint_property_checks() {
        let schema = Ontology::new().with_axiom(Axiom::DisjointDataProperties(vec![
            term("same-age"),
            term("same-height"),
        ]));
        let base = schema
            .clone()
            .with_axiom(fact("same-disjoint-a", "same-age", Literal::Integer(30)))
            .with_axiom(fact(
                "same-disjoint-b",
                "same-height",
                Literal::Integer(180),
            ));

        assert!(validate_data_consistency(&base).is_ok());
        let mut state = DataConsistencyState::from_ontology(&base).unwrap();
        assert!(
            state
                .insert_delta(&Axiom::SameIndividuals(vec![
                    term("same-disjoint-a"),
                    term("same-disjoint-b"),
                ]))
                .is_err()
        );
    }

    #[test]
    fn appended_subproperty_tbox_revalidates_retained_assertions() {
        let sub = Axiom::SubDataPropertyOf {
            sub: term("delta-sub-work"),
            sup: term("delta-sub-email"),
        };
        let base = Ontology::new()
            .with_axiom(Axiom::DisjointDataProperties(vec![
                term("delta-sub-email"),
                term("delta-sub-phone"),
            ]))
            .with_axiom(fact(
                "delta-sub-person",
                "delta-sub-work",
                Literal::String("one".into()),
            ))
            .with_axiom(fact(
                "delta-sub-person",
                "delta-sub-phone",
                Literal::String("555-1000".into()),
            ));

        assert!(is_incremental_tbox_axiom(&sub));
        let mut state = DataConsistencyState::from_ontology(&base).unwrap();
        assert!(state.insert_delta(&sub).is_err());

        let extended = base.with_axiom(sub);
        assert!(validate_data_consistency(&extended).is_err());
    }

    #[test]
    fn assertion_patch_additions_participate_in_later_tbox_expansion() {
        let property = term("patch-add-work");
        let schema = Ontology::new().with_axiom(Axiom::FunctionalDataProperty(property.clone()));
        let base = schema.clone().with_axiom(fact(
            "patch-add-person",
            "patch-add-work",
            Literal::String("old".into()),
        ));
        let next = schema.clone().with_axiom(fact(
            "patch-add-person",
            "patch-add-work",
            Literal::String("new".into()),
        ));

        let mut state = DataConsistencyState::from_ontology(&base).unwrap();
        let patch = assertion_delta(&base, &next).unwrap();
        for fact in state.expand_all(&patch.remove_positive) {
            assert!(state.remove_positive(&fact).is_some());
        }
        assert!(state.forget_removed_raw_assertions(&patch).is_some());
        assert!(
            state
                .insert_added_positive(DataFact {
                    individual: term("patch-add-person"),
                    property,
                    value: Literal::String("new".into()),
                })
                .is_ok()
        );
        assert_eq!(1, state.raw_positives.len());

        for axiom in [
            Axiom::SubDataPropertyOf {
                sub: term("patch-add-work"),
                sup: term("patch-add-email"),
            },
            Axiom::SubDataPropertyOf {
                sub: term("patch-add-email"),
                sup: term("patch-add-contact"),
            },
        ] {
            assert!(state.insert_delta(&axiom).is_ok());
        }
        assert!(
            state
                .positives
                .iter()
                .any(|fact| fact.property == term("patch-add-contact"))
        );

        let range = Axiom::DataPropertyDataRange {
            property: term("patch-add-contact"),
            range: DataRange::Datatype(Datatype::Integer),
        };
        assert!(state.insert_delta(&range).is_err());
    }

    #[test]
    fn assertion_patches_forget_hierarchy_derived_facts() {
        let schema = Ontology::new()
            .with_axiom(Axiom::SubDataPropertyOf {
                sub: term("patch-raw-work"),
                sup: term("patch-raw-email"),
            })
            .with_axiom(Axiom::FunctionalDataProperty(term("patch-raw-email")));
        let base = schema.clone().with_axiom(fact(
            "patch-raw-person",
            "patch-raw-work",
            Literal::String("same".into()),
        ));
        let removed = schema.clone();
        let negated = schema.clone().with_axiom(negative(
            "patch-raw-person",
            "patch-raw-email",
            Literal::String("same".into()),
        ));

        let mut state = DataConsistencyState::from_ontology(&base).unwrap();
        let patch = assertion_delta(&base, &removed).unwrap();
        let expanded = state.expand_all(&state.raw_positives.clone());
        assert_eq!(2, expanded.len());
        assert!(state.forget_removed_raw_assertions(&patch).is_some());
        for fact in expanded {
            assert!(state.remove_positive(&fact).is_some());
        }
        assert!(state.raw_positives.is_empty());
        assert!(
            state
                .insert_negative(DataFact {
                    individual: term("patch-raw-person"),
                    property: term("patch-raw-email"),
                    value: Literal::String("same".into()),
                })
                .is_ok()
        );

        // Exercise the same cache transition end-to-end so a retained raw fact
        // cannot survive the removal and falsely reject its later negation.
        assert!(validate_data_consistency(&base).is_ok());
        let with_subproperty = base;
        assert!(validate_data_consistency(&with_subproperty).is_ok());
        assert!(validate_data_consistency(&removed).is_ok());
        assert!(validate_data_consistency(&negated).is_ok());
    }

    #[test]
    fn appended_tbox_constraints_revalidate_retained_assertions() {
        let cases = vec![
            (
                Ontology::new()
                    .with_axiom(fact(
                        "tbox-functional-a",
                        "tbox-status",
                        Literal::String("first".into()),
                    ))
                    .with_axiom(fact(
                        "tbox-functional-a",
                        "tbox-status",
                        Literal::String("second".into()),
                    )),
                Axiom::FunctionalDataProperty(term("tbox-status")),
            ),
            (
                Ontology::new()
                    .with_axiom(fact("tbox-disjoint-a", "tbox-age", Literal::Integer(30)))
                    .with_axiom(fact(
                        "tbox-disjoint-a",
                        "tbox-height",
                        Literal::Integer(180),
                    )),
                Axiom::DisjointDataProperties(vec![term("tbox-age"), term("tbox-height")]),
            ),
            (
                Ontology::new().with_axiom(fact("tbox-range-a", "tbox-name", Literal::Integer(30))),
                Axiom::DataPropertyRange {
                    property: term("tbox-name"),
                    datatype: Datatype::String,
                },
            ),
            (
                Ontology::new().with_axiom(fact(
                    "tbox-one-of-a",
                    "tbox-state",
                    Literal::String("rejected".into()),
                )),
                Axiom::DataPropertyOneOf {
                    property: term("tbox-state"),
                    values: vec![Literal::String("approved".into())],
                },
            ),
            (
                Ontology::new().with_axiom(fact(
                    "tbox-pattern-a",
                    "tbox-code",
                    Literal::String("bad".into()),
                )),
                Axiom::DataPropertyRangeRestriction {
                    property: term("tbox-code"),
                    datatype: Datatype::String,
                    facets: vec![FacetValue {
                        facet: DataFacet::Pattern,
                        value: Literal::String("^good$".into()),
                    }],
                },
            ),
            (
                Ontology::new().with_axiom(fact(
                    "tbox-data-range-a",
                    "tbox-count",
                    Literal::Boolean(true),
                )),
                Axiom::DataPropertyDataRange {
                    property: term("tbox-count"),
                    range: DataRange::Datatype(Datatype::Integer),
                },
            ),
        ];

        for (base, tbox) in cases {
            assert!(validate_data_consistency(&base).is_ok());
            let mut state = DataConsistencyState::from_ontology(&base).unwrap();
            assert!(
                state.apply_tbox_delta(&tbox).is_err(),
                "appended {tbox:?} must revalidate retained facts"
            );

            let extended = base.clone().with_axiom(tbox.clone());
            let prefix_length = data_consistency_projection(&base).len();
            assert!(supports_incremental_suffix(
                &data_consistency_projection(&extended)[prefix_length..]
            ));
            assert_eq!(
                data_consistency_projection(&extended).len(),
                prefix_length + 1
            );
            assert!(validate_data_consistency(&extended).is_err());
        }
    }

    #[test]
    fn appended_functional_tbox_applies_to_later_assertions() {
        let base = Ontology::new().with_axiom(fact(
            "tbox-later-a",
            "tbox-later-status",
            Literal::String("active".into()),
        ));
        let tbox = Axiom::FunctionalDataProperty(term("tbox-later-status"));
        assert!(validate_data_consistency(&base).is_ok());

        let mut state = DataConsistencyState::from_ontology(&base).unwrap();
        assert!(state.apply_tbox_delta(&tbox).is_ok());
        assert!(
            state
                .insert_delta(&fact(
                    "tbox-later-a",
                    "tbox-later-status",
                    Literal::String("inactive".into()),
                ))
                .is_err()
        );

        let valid = base.clone().with_axiom(tbox).with_axiom(fact(
            "tbox-later-c",
            "tbox-later-status",
            Literal::String("active".into()),
        ));
        let prefix_length = data_consistency_projection(&base).len();
        assert!(supports_incremental_suffix(
            &data_consistency_projection(&valid)[prefix_length..]
        ));
        assert!(validate_data_consistency(&valid).is_ok());
    }

    #[test]
    fn assertion_patch_keys_preserve_distinct_far_future_date_times() {
        let property = term("patch-instant");
        let base = Ontology::new()
            .with_axiom(Axiom::FunctionalDataProperty(property.clone()))
            .with_axiom(fact(
                "patch-dt-a",
                "patch-instant",
                Literal::DateTime("3000-01-01T00:00:00Z".into()),
            ));
        assert!(validate_data_consistency(&base).is_ok());

        // The old positive value is outside `timestamp_nanos_opt` range.  Its
        // patch key must still remain distinct from 1970 so it cannot cancel
        // the replacement and hide this next snapshot's direct contradiction.
        let next = Ontology::new()
            .with_axiom(Axiom::FunctionalDataProperty(property))
            .with_axiom(fact(
                "patch-dt-a",
                "patch-instant",
                Literal::DateTime("1970-01-01T00:00:00Z".into()),
            ))
            .with_axiom(negative(
                "patch-dt-a",
                "patch-instant",
                Literal::DateTime("1970-01-01T00:00:00Z".into()),
            ));
        assert!(validate_data_consistency(&next).is_err());
    }

    #[test]
    fn removing_one_fact_retains_other_properties_in_disjoint_checks() {
        let schema = Ontology::new().with_axiom(Axiom::DisjointDataProperties(vec![
            term("patch-age"),
            term("patch-height"),
        ]));
        let base = schema
            .clone()
            .with_axiom(fact("patch-index-a", "patch-age", Literal::Integer(30)))
            .with_axiom(fact("patch-index-a", "patch-temp", Literal::Integer(80)));
        assert!(validate_data_consistency(&base).is_ok());

        let next = schema
            .clone()
            .with_axiom(fact("patch-index-a", "patch-age", Literal::Integer(30)))
            .with_axiom(fact("patch-index-a", "patch-height", Literal::Integer(180)));
        assert!(
            validate_data_consistency(&next).is_err(),
            "removing patch-temp must not forget the retained patch-age property"
        );
    }

    #[test]
    fn replacement_uses_an_assertion_patch_and_avoids_stale_conflicts() {
        let schema =
            Ontology::new().with_axiom(Axiom::FunctionalDataProperty(term("patch-replace-status")));
        let base = schema.clone().with_axiom(fact(
            "patch-replace-a",
            "patch-replace-status",
            Literal::Integer(30),
        ));
        let next = schema.clone().with_axiom(fact(
            "patch-replace-a",
            "patch-replace-status",
            Literal::Integer(31),
        ));

        let state = DataConsistencyState::from_ontology(&base).unwrap();
        data_consistency_cache().lock().expect("cache mutex").put(
            ontology_fingerprint(&base),
            &base,
            state,
            Ok(()),
        );

        let candidate = data_consistency_cache()
            .lock()
            .expect("cache mutex")
            .find_patch_base(&next);
        let Some((entry, patch)) = candidate else {
            panic!("same-schema fact replacement should reuse a patch base");
        };
        assert_eq!(1, patch.remove_positive.len());
        assert_eq!(1, patch.add_positive.len());
        assert!(patch.remove_negative.is_empty());
        assert!(patch.add_negative.is_empty());
        data_consistency_cache()
            .lock()
            .expect("cache mutex")
            .restore(entry);

        assert!(validate_data_consistency(&next).is_ok());
    }

    #[test]
    fn deleting_a_positive_assertion_does_not_create_a_negative() {
        let schema = Ontology::new();
        let base = schema
            .clone()
            .with_axiom(fact("patch-delete-a", "patch-age", Literal::Integer(30)))
            .with_axiom(fact("patch-delete-a", "patch-temp", Literal::Integer(80)));
        let next = schema.with_axiom(fact("patch-delete-a", "patch-age", Literal::Integer(30)));

        let patch = assertion_delta(&base, &next).unwrap();
        assert_eq!(1, patch.remove_positive.len());
        assert!(patch.add_positive.is_empty());
        assert!(patch.remove_negative.is_empty());
        assert!(patch.add_negative.is_empty());
        assert!(validate_data_consistency(&next).is_ok());
    }

    #[test]
    fn assertion_delta_preserves_duplicate_multiplicity() {
        let one = Ontology::from_iter([fact(
            "patch-multi-a",
            "patch-tag",
            Literal::String("x".into()),
        )]);
        let two = Ontology::from_iter([
            fact("patch-multi-a", "patch-tag", Literal::String("x".into())),
            fact("patch-multi-a", "patch-tag", Literal::String("x".into())),
        ]);

        let added = assertion_delta(&one, &two).unwrap();
        assert_eq!(1, added.add_positive.len());
        assert!(added.remove_positive.is_empty());

        let removed = assertion_delta(&two, &one).unwrap();
        assert_eq!(1, removed.remove_positive.len());
        assert!(removed.add_positive.is_empty());

        let unchanged = assertion_delta(&two, &two).unwrap();
        assert!(unchanged.add_positive.is_empty());
        assert!(unchanged.remove_positive.is_empty());
    }

    #[test]
    fn same_individual_changes_do_not_reuse_an_assertion_patch() {
        let base =
            Ontology::new().with_axiom(fact("patch-identity-a", "patch-age", Literal::Integer(30)));
        let changed_identity = base.clone().with_axiom(Axiom::SameIndividuals(vec![
            term("patch-identity-a"),
            term("patch-identity-b"),
        ]));

        let state = DataConsistencyState::from_ontology(&base).unwrap();
        data_consistency_cache().lock().expect("cache mutex").put(
            ontology_fingerprint(&base),
            &base,
            state,
            Ok(()),
        );

        let candidate = data_consistency_cache()
            .lock()
            .expect("cache mutex")
            .find_patch_base(&changed_identity);
        assert!(candidate.is_none());
        assert!(!same_data_schema(&base, &changed_identity));
    }

    #[test]
    fn prefix_equality_requires_the_same_projection_length() {
        let retained = Ontology::new().with_axiom(fact("a", "tag", Literal::String("kept".into())));
        let extended =
            retained
                .clone()
                .with_axiom(fact("b", "tag", Literal::String("kept".into())));
        let query = data_consistency_projection(&extended);

        assert!(projection_prefix_equals(
            &retained,
            &data_consistency_projection(&retained)
        ));
        assert!(!projection_prefix_equals(&retained, &query));
    }

    #[test]
    fn appended_negative_and_conflicting_positive_assertions_fail_closed() {
        let schema = Ontology::new().with_axiom(Axiom::DataPropertyRange {
            property: term("age"),
            datatype: Datatype::Integer,
        });
        let positive = schema
            .clone()
            .with_axiom(fact("alice", "age", Literal::Integer(30)));
        assert!(validate_data_consistency(&positive).is_ok());

        let negated = positive.with_axiom(negative("alice", "age", Literal::Integer(30)));
        assert!(validate_data_consistency(&negated).is_err());

        // The reverse append order must reach the same contradiction.
        let negative_first = schema
            .with_axiom(negative("bob", "age", Literal::Integer(31)))
            .with_axiom(fact("bob", "age", Literal::Integer(31)));
        assert!(validate_data_consistency(&negative_first).is_err());
    }

    #[test]
    fn fingerprint_collision_is_treated_as_a_cache_miss() {
        let retained = Ontology::new().with_axiom(fact("a", "tag", Literal::String("kept".into())));
        let candidate =
            Ontology::new().with_axiom(fact("a", "tag", Literal::String("other".into())));
        let mut cache = DataConsistencyLru::new(1);

        cache.put(42, &retained, DataConsistencyState::default(), Ok(()));
        assert!(cache.get(42, &candidate).is_none());
        cache.put(42, &candidate, DataConsistencyState::default(), Ok(()));

        assert_eq!(1, cache.len());
        assert!(!cache.contains_key(42, &retained));
        assert_eq!(
            Some(Ok(())),
            cache.get(42, &candidate).map(|(result, _)| result)
        );
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
