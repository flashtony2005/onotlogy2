//! Core-path performance baselines for DoD D7.
//!
//! The scenarios are deliberately fixed so results remain comparable:
//! - ODL compile: 24 object types, each with one action.
//! - Projection replay: 1,000 deterministic world events.
//! - World query: 500 persisted objects (the engine query limit is 1,000).
//! - Audit write: one append-only event and hash-chain node per iteration.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};

use criterion::{
    BatchSize, Criterion, SamplingMode, Throughput, black_box, criterion_group, criterion_main,
};
use openaip_os::abi::{codegen, parse_odl};
use openaip_os::kernel::{Kernel, Scope};
use openaip_os::knowledge::{
    Axiom, ClassExpression, Literal, Ontology, Reasoner, Term, validate_data_consistency,
};
use openaip_os::projection::Projection;
use openaip_os::storage::{LibSQLStorage, ObjectEngine};
use openaip_os::trace::{TraceEvent, TraceKind};
use serde_json::json;

const ODL_OBJECTS: usize = 24;
const REASONER_CLASSES: usize = 24;
const DATA_FACTS: usize = 1_000;
const CONCURRENT_THREADS: usize = 8;
const CONCURRENT_CALLS_PER_THREAD: usize = 128;
const PROJECTION_EVENTS: usize = 1_000;
const QUERY_OBJECTS: usize = 500;

fn benchmark_odl() -> String {
    let mut odl = String::new();
    for index in 0..ODL_OBJECTS {
        odl.push_str(&format!(
            r#"
object Item{index}:
  properties:
    name: string
    score: int
  actions:
    complete:
      updates:
        - field: score
          value: 100
"#
        ));
    }
    odl
}

/// A fixed named-class taxonomy large enough to expose cache overhead/benefit.
fn benchmark_ontology() -> Ontology {
    let mut ontology = Ontology::new();
    for index in 0..REASONER_CLASSES {
        let sub = Term::new(format!("Item{index}"));
        let sup = if index + 1 == REASONER_CLASSES {
            ClassExpression::Thing
        } else {
            ClassExpression::class(Term::new(format!("Item{}", index + 1)))
        };
        ontology = ontology.with_axiom(Axiom::subclass(ClassExpression::Class(sub), sup));
    }
    ontology
}

/// A fixed functional-property ABox that exercises indexed conflict detection.
fn benchmark_data_ontology() -> Ontology {
    let ontology = Ontology::new().with_axiom(Axiom::FunctionalDataProperty(Term::new("status")));
    (0..DATA_FACTS).fold(ontology, |ontology, index| {
        ontology.with_axiom(Axiom::DataPropertyAssertion {
            property: Term::new("status"),
            individual: Term::new(format!("individual-{index}")),
            value: Literal::String("active".into()),
        })
    })
}

fn projection_events() -> Vec<TraceEvent> {
    (0..PROJECTION_EVENTS)
        .map(|index| {
            let id = format!("bench:{index}");
            let timestamp = ((index + 1) * 1_000_000).to_string();
            if index % 4 == 3 {
                TraceEvent {
                    id,
                    kind: TraceKind::World,
                    event_type: "RelationLinked".into(),
                    object_refs: vec![format!("obj:{}", index - 2), format!("obj:{}", index)],
                    project_id: None,
                    run_id: None,
                    agent_id: None,
                    reasoning: None,
                    input: Some(json!({ "edge": "related" })),
                    output: None,
                    confidence: None,
                    timestamp,
                }
            } else {
                TraceEvent {
                    id,
                    kind: TraceKind::World,
                    event_type: "ObjectCreated".into(),
                    object_refs: vec![format!("obj:{index}")],
                    project_id: None,
                    run_id: None,
                    agent_id: None,
                    reasoning: None,
                    input: Some(json!({
                        "type": "Item",
                        "data": { "name": format!("item-{index}"), "score": index }
                    })),
                    output: None,
                    confidence: None,
                    timestamp,
                }
            }
        })
        .collect()
}

async fn seed_query_storage(storage: &LibSQLStorage) {
    for index in 0..QUERY_OBJECTS {
        storage
            .create(
                "BenchItem",
                &format!("query:{index}"),
                json!({ "name": format!("item-{index}"), "score": index }),
            )
            .await
            .expect("seed benchmark object");
    }
}

fn audit_event(sequence: u64) -> TraceEvent {
    TraceEvent {
        id: format!("audit-bench:{sequence}"),
        kind: TraceKind::World,
        event_type: "ObjectCreated".into(),
        object_refs: vec![format!("audit-obj:{sequence}")],
        project_id: None,
        run_id: None,
        agent_id: None,
        reasoning: None,
        input: Some(json!({ "type": "AuditItem", "data": { "sequence": sequence } })),
        output: None,
        confidence: None,
        timestamp: openaip_os::trace::now_ts(),
    }
}

fn core_performance(c: &mut Criterion) {
    let mut group = c.benchmark_group("core_paths");
    group.sampling_mode(SamplingMode::Flat);
    group.sample_size(30);

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("benchmark Tokio runtime");

    let source = benchmark_odl();
    group.throughput(Throughput::Elements(ODL_OBJECTS as u64));
    group.bench_function("odl_compile_24_objects", |b| {
        b.iter(|| {
            let ir = parse_odl(black_box(&source)).expect("parse ODL");
            let output = codegen(&ir).expect("generate ODL artifacts");
            black_box(output.rust.len() + output.ontology_json.len())
        })
    });

    group.throughput(Throughput::Elements(REASONER_CLASSES as u64));
    group.bench_function("reasoner_classification_new_24_classes", |b| {
        let iteration = AtomicU64::new(0);
        b.iter(|| {
            let sequence = iteration.fetch_add(1, Ordering::Relaxed);
            // A unique ABox individual keeps this benchmark on the cold path;
            // process-level snapshot reuse would otherwise hide conversion cost.
            let ontology = benchmark_ontology().with_axiom(Axiom::ClassAssertion {
                class: ClassExpression::Thing,
                individual: Term::new(format!("cold-{sequence}")),
            });
            let reasoner = Reasoner::new(black_box(&ontology)).expect("create reasoner");
            black_box(reasoner.classify().expect("classify").classes().len())
        })
    });
    let ontology = benchmark_ontology();
    let reasoner = Reasoner::new(&ontology).expect("warm reasoner");
    reasoner.classify().expect("warm classification");
    group.bench_function("reasoner_classification_cached_24_classes", |b| {
        b.iter(|| {
            black_box(
                reasoner
                    .classify()
                    .expect("cached classify")
                    .classes()
                    .len(),
            )
        })
    });

    let events = projection_events();
    group.throughput(Throughput::Elements(events.len() as u64));
    group.bench_function("projection_replay_1000_events", |b| {
        b.iter(|| {
            let mut kernel = Kernel::new();
            let context = kernel
                .create_context(None, Scope::World, "world")
                .expect("create kernel context");
            let mut projection = Projection::new(context);
            let applied = projection
                .project_events(&mut kernel, black_box(&events))
                .expect("project events");
            black_box(applied)
        })
    });

    let storage = runtime
        .block_on(LibSQLStorage::open(":memory:"))
        .expect("open in-memory database");
    runtime.block_on(seed_query_storage(&storage));
    group.throughput(Throughput::Elements(QUERY_OBJECTS as u64));
    group.bench_function("world_query_500_objects", |b| {
        b.iter(|| {
            let records = runtime
                .block_on(storage.query("BenchItem"))
                .expect("query world objects");
            black_box(records.len())
        })
    });

    let audit_sequence = AtomicU64::new(0);
    group.throughput(Throughput::Elements(1));
    group.bench_function("audit_write_1_event", |b| {
        b.iter(|| {
            let sequence = audit_sequence.fetch_add(1, Ordering::Relaxed);
            runtime
                .block_on(storage.append_trace(&audit_event(sequence)))
                .expect("append audit event");
            black_box(sequence)
        })
    });

    group.finish();
}

fn knowledge_abox_performance(c: &mut Criterion) {
    let ontology = benchmark_data_ontology();
    assert!(validate_data_consistency(&ontology).is_ok());
    let cold_iteration = AtomicU64::new(0);
    let delta_iteration = AtomicU64::new(0);
    let delta_snapshot = RefCell::new(ontology.clone());

    let mut group = c.benchmark_group("knowledge_abox");
    group.sample_size(10);
    group.throughput(Throughput::Elements(DATA_FACTS as u64));
    group.bench_function("data_consistency_1000_assertions", |b| {
        b.iter_batched(
            || {
                let sequence = cold_iteration.fetch_add(1, Ordering::Relaxed);
                // A unique, inert SameIndividuals axiom is data-relevant but
                // too small to create an alias.  It forces a fresh index
                // instead of an append-only prefix or exact-result cache hit.
                ontology
                    .clone()
                    .with_axiom(Axiom::SameIndividuals(vec![Term::new(format!(
                        "cold-{sequence}"
                    ))]))
            },
            |cold| black_box(validate_data_consistency(black_box(&cold)).is_ok()),
            BatchSize::PerIteration,
        )
    });
    assert!(validate_data_consistency(&ontology).is_ok());
    group.bench_function("data_consistency_cached_1000_assertions", |b| {
        b.iter(|| black_box(validate_data_consistency(black_box(&ontology)).is_ok()))
    });

    // The cold loop deliberately evicts the base snapshot from the small LRU.
    // Re-warm it so this group isolates append-only reuse from unrelated cache
    // churn caused by measuring the full-rebuild baseline in the same process.
    assert!(validate_data_consistency(&ontology).is_ok());

    // Measure a bounded append-only chain: the previous validated snapshot is
    // the next prefix.  Distinct siblings are never prefixes of each other, so
    // repeatedly appending to base would measure LRU churn and full rebuilds
    // instead of delta reuse.  Reset after the fixed workload so Criterion's
    // warmup cannot silently grow the ontology into a different benchmark.
    group.throughput(Throughput::Elements(1));
    group.bench_function("data_consistency_delta_append_chain", |b| {
        b.iter_batched(
            || {
                let sequence = delta_iteration.fetch_add(1, Ordering::Relaxed);
                if sequence.is_multiple_of(DATA_FACTS as u64) {
                    delta_snapshot.replace(ontology.clone());
                }
                let next =
                    delta_snapshot
                        .borrow()
                        .clone()
                        .with_axiom(Axiom::DataPropertyAssertion {
                            property: Term::new("status"),
                            individual: Term::new(format!("delta-{sequence}")),
                            value: Literal::String("active".into()),
                        });
                delta_snapshot.replace(next.clone())
            },
            |extended| black_box(validate_data_consistency(black_box(&extended)).is_ok()),
            BatchSize::PerIteration,
        )
    });

    // The cold/delta loops above intentionally churn the process-local LRU.
    // Re-warm the shared snapshot so the burst measures cached concurrent
    // validation rather than eviction caused by the preceding benchmarks.
    assert!(validate_data_consistency(&ontology).is_ok());
    let calls_per_burst = CONCURRENT_THREADS * CONCURRENT_CALLS_PER_THREAD;
    group.throughput(Throughput::Elements(calls_per_burst as u64));
    group.bench_function("data_consistency_concurrent_cached_8x128", |b| {
        b.iter(|| {
            let results = std::thread::scope(|scope| {
                (0..CONCURRENT_THREADS)
                    .map(|_| {
                        scope.spawn(|| {
                            (0..CONCURRENT_CALLS_PER_THREAD).all(|_| {
                                black_box(validate_data_consistency(black_box(&ontology)).is_ok())
                            })
                        })
                    })
                    .collect::<Vec<_>>()
                    .into_iter()
                    .map(|handle| handle.join().expect("concurrent consistency worker"))
                    .collect::<Vec<_>>()
            });
            for result in results {
                black_box(result);
            }
        })
    });
    group.finish();
}

criterion_group!(benches, core_performance, knowledge_abox_performance);
criterion_main!(benches);
