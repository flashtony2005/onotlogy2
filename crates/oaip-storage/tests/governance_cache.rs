use oaip_knowledge::{Axiom, ClassExpression, Ontology, OntologyChange, OntologyProposal, Term};
use oaip_storage::{LibSQLStorage, ObjectEngine};
use serde_json::json;

fn class(name: &str) -> ClassExpression {
    ClassExpression::class(format!("http://example.com/owl#{name}"))
}

/// Keep this cache-shape assertion in its own integration-test process.
/// `validate_data_consistency` intentionally owns a process-local LRU, so
/// parallel unit tests otherwise can warm the same snapshots and change exact
/// hit counts without any governance-path behavior changing.
#[tokio::test]
async fn governance_execution_reuses_cached_data_prefixes_and_exact_results() {
    oaip_knowledge::reset_data_consistency_thread_stats();
    let storage = LibSQLStorage::open(":memory:").await.unwrap();

    let root = Ontology::new().with_axiom(Axiom::FunctionalDataProperty(Term::new("age")));
    storage
        .initialize_knowledge_version("root", &root, None)
        .await
        .unwrap();
    storage
        .create("Customer", "customer-1", json!({ "age": 30 }))
        .await
        .unwrap();

    let first = OntologyProposal::new("add-dog").with_change(OntologyChange::Add(Axiom::subclass(
        class("Dog"),
        class("Animal"),
    )));
    storage
        .propose_knowledge_change("root", &first, None)
        .await
        .unwrap();
    storage
        .decide_knowledge_proposal("add-dog", true, "alice", None)
        .await
        .unwrap();
    storage
        .apply_knowledge_proposal("add-dog", "v2")
        .await
        .unwrap();

    // The new live assertion extends the previously validated data projection,
    // while the rewritten DifferentIndividuals tail is inert.
    storage
        .create("Customer", "customer-2", json!({ "age": 31 }))
        .await
        .unwrap();
    let second = OntologyProposal::new("add-puppy").with_change(OntologyChange::Add(
        Axiom::subclass(class("Puppy"), class("Dog")),
    ));
    storage
        .propose_knowledge_change("v2", &second, None)
        .await
        .unwrap();
    storage
        .decide_knowledge_proposal("add-puppy", true, "bob", None)
        .await
        .unwrap();
    storage
        .apply_knowledge_proposal("add-puppy", "v3")
        .await
        .unwrap();

    let stats = oaip_knowledge::data_consistency_thread_stats();
    // Governance also validates proposal impact on source/target snapshots,
    // so the totals include schema-only Reasoner warmup traffic. The fixed
    // shape proves live-fact appends and replacements reuse a cached prefix or
    // patch base while repeated applied snapshots hit the completed cache.
    assert_eq!(
        oaip_knowledge::DataConsistencyCacheStats {
            exact_hits: 3,
            prefix_hits: 6,
            rebuilds: 1,
            delta_assertions: 8,
        },
        stats
    );
}
