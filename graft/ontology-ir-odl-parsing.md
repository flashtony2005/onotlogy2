---
name: Ontology IR & ODL Parsing
slug: ontology-ir-odl-parsing
type: system
sources:
  - path: src/ir.rs
    hash: 450ff5fd7028f0c4fcb9bdbbeda3fa6c93b7c555934b5eb19d2838a598e4647f
  - path: src/odl.rs
    hash: 234949cb0e91f44545f73701498000fb405d678202f2ce51b583506d75f2d169
sources_digest: f29249acf6c660265e2bf0610f1d20b72925fe8d520ab5dd98aa12cda6a4c5b2
links:
  - to: action-engine
    relation: produces
    description: ActionRegistry builds its type-to-action map from OntologyIR ActionDefs.
  - to: code-generation
    relation: produces
    description: 'codegen consumes OntologyIR to emit Rust structs, SQL schema, and JSON.'
generator:
  version: 1
covers: []
---
<!-- context:generated:start -->
## Summary

The language-independent intermediate representation and its deterministic parser. `ir.rs` defines the data structures (ObjectType, ActionDef, PolicyDef, ValueExpr) that everything downstream consumes; `odl.rs` is a hand-written indentation-based state machine converting .odl text into that IR, deliberately avoiding LLM-based parsing and enforcing constraints like 'every object needs ≥1 property' (error 1001). The IR is the single contract between authoring and execution.

## Related

- produces [[action-engine]] — ActionRegistry builds its type-to-action map from OntologyIR ActionDefs.
- produces [[code-generation]] — codegen consumes OntologyIR to emit Rust structs, SQL schema, and JSON.
<!-- context:generated:end -->

## Notes

_Anything written below the generated block is preserved when the graph is regenerated._
