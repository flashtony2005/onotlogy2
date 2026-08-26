//! Controlled reasoning scalability probe.
//!
//! Measures the full pipeline: object fact generation → knowledge-base conversion →
//! Reasoner construction → consistency checking, at configurable ABox sizes.
//! Run with:
//!   cargo test --release --bench reasoner-scale -- --nocapture

use std::collections::BTreeMap;
use std::time::Instant;

use openaip_os::knowledge::{
    Axiom, ClassExpression, Ontology, Reasoner, RuntimeObjectFact, Term, runtime_world_to_knowledge,
};

fn probe_ontology() -> Ontology {
    // Minimal TBox: one class declaration via ClassAssertion usage pattern,
    // a functional data property (the construct that motivates global distinctness),
    // and a subclass axiom so classification has work.
    Ontology::new()
        .with_axiom(Axiom::FunctionalDataProperty(Term::new("status")))
        .with_axiom(Axiom::SubClassOf {
            sub: ClassExpression::Class(Term::new("ProbeEntity")),
            sup: ClassExpression::Thing,
        })
}

fn make_objects(count: usize) -> Vec<RuntimeObjectFact> {
    (0..count)
        .map(|i| RuntimeObjectFact {
            id: format!("probe-{i}"),
            object_type: "ProbeEntity".to_string(),
            data: serde_json::json!({"status": "active"}),
        })
        .collect()
}

struct PhaseTiming {
    label: &'static str,
    duration_ms: f64,
}

fn run_probe(scale: usize) -> (f64, f64) {
    let mut timings = Vec::<PhaseTiming>::new();

    let t0 = Instant::now();
    let ontology = probe_ontology();
    let objects = make_objects(scale);
    timings.push(PhaseTiming {
        label: "fact_generation",
        duration_ms: t0.elapsed().as_secs_f64() * 1000.0,
    });

    let t1 = Instant::now();
    let relations: Vec<openaip_os::knowledge::RuntimeRelationFact> = vec![];
    let kb = runtime_world_to_knowledge(&ontology, &objects, &relations)
        .unwrap_or_else(|e| panic!("conversion failed at {scale}: {e}"));
    timings.push(PhaseTiming {
        label: "runtime_world_to_knowledge",
        duration_ms: t1.elapsed().as_secs_f64() * 1000.0,
    });
    println!("  KB axioms: {}", kb.axioms().len());

    let t2 = Instant::now();
    let reasoner_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Reasoner::new(&kb).map(|r| r.is_consistent())
    }));
    match reasoner_result {
        Ok(Ok(Ok(is_consistent))) => {
            timings.push(PhaseTiming {
                label: "reasoner_new_plus_consistency",
                duration_ms: t2.elapsed().as_secs_f64() * 1000.0,
            });
            println!("  consistent: {is_consistent}");
        }
        Ok(Ok(Err(e))) => {
            timings.push(PhaseTiming {
                label: "reasoner_error",
                duration_ms: t2.elapsed().as_secs_f64() * 1000.0,
            });
            println!("  reasoner error: {e}");
        }
        Ok(Err(_)) | Err(_) => {
            timings.push(PhaseTiming {
                label: "reasoner_panic",
                duration_ms: t2.elapsed().as_secs_f64() * 1000.0,
            });
        }
    }

    let total: f64 = timings.iter().map(|t| t.duration_ms).sum();
    println!("  total: {:.1}ms", total);
    for t in &timings {
        println!("  {}: {:.1}ms", t.label, t.duration_ms);
    }
    (
        timings
            .iter()
            .find(|timing| timing.label == "reasoner_new_plus_consistency")
            .map(|timing| timing.duration_ms)
            .unwrap_or(f64::INFINITY),
        total,
    )
}

fn main() {
    let scales = [100usize, 500, 1_000, 5_000];
    let timings = scales
        .iter()
        .map(|scale| {
            println!("\n=== scale={scale} ===");
            (*scale, run_probe(*scale))
        })
        .collect::<BTreeMap<_, _>>();

    if let Ok(sla_ms) = std::env::var("REASONER_SCALE_SLA_MS") {
        let sla_ms = sla_ms.parse::<f64>().expect("SLA must be milliseconds");
        let (reasoning_ms, total_ms) = timings[&5_000];
        println!(
            "\n5k reasoning SLA: {reasoning_ms:.1}ms <= {sla_ms}ms (full pipeline: {total_ms:.1}ms)"
        );
        assert!(
            reasoning_ms <= sla_ms,
            "5k reasoning took {reasoning_ms:.1}ms, exceeding the {sla_ms}ms SLA"
        );
    }
}
