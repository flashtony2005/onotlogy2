//! Core-path performance baselines for DoD D7.
//!
//! The scenarios are deliberately fixed so results remain comparable:
//! - ODL compile: 24 object types, each with one action.
//! - Projection replay: 1,000 deterministic world events.
//! - World query: 500 persisted objects (the engine query limit is 1,000).
//! - Audit write: one append-only event and hash-chain node per iteration.

use std::sync::atomic::{AtomicU64, Ordering};

use criterion::{Criterion, SamplingMode, Throughput, black_box, criterion_group, criterion_main};
use openaip_os::abi::{codegen, parse_odl};
use openaip_os::kernel::{Kernel, Scope};
use openaip_os::knowledge::{Axiom, ClassExpression, Ontology, Reasoner, Term};
use openaip_os::projection::Projection;
use openaip_os::storage::{LibSQLStorage, ObjectEngine};
use openaip_os::trace::{TraceEvent, TraceKind};
use serde_json::json;

const ODL_OBJECTS: usize = 24;
const REASONER_CLASSES: usize = 24;
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

    let ontology = benchmark_ontology();
    group.throughput(Throughput::Elements(REASONER_CLASSES as u64));
    group.bench_function("reasoner_classification_new_24_classes", |b| {
        b.iter(|| {
            let reasoner = Reasoner::new(black_box(&ontology)).expect("create reasoner");
            black_box(reasoner.classify().expect("classify").classes().len())
        })
    });
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

criterion_group!(benches, core_performance);
criterion_main!(benches);
