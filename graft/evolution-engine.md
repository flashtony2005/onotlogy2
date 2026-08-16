---
name: Evolution Engine
slug: evolution-engine
type: system
sources:
  - path: src/evolution.rs
    hash: 8b22e1db63cf8cb31cf6cdc3f9664b067373f58a2c20046e9cfc21657c44cef0
sources_digest: 8533edaaaa60901516c990d942152cd4a38c83500e7b061b379cfe7b2ecc1506
links:
  - to: persistence-layer
    relation: uses
    description: >-
      Evolution artifacts (patterns, proposals, learned knowledge) stored via
      LibSQLStorage.
  - to: runtime-kernel-observability
    relation: uses
    description: >-
      run_evolution drives the pipeline; kernel persists learned knowledge
      idempotently.
generator:
  version: 1
covers: []
---
<!-- context:generated:start -->
## Summary

The deterministic self-learning pipeline (BL-003 · REQ-017) converting agent experiences from wb_memory into validated knowledge through five stages: Experience → Pattern → Proposal → Validation → LearnedKnowledge, each carrying traceable parent references. Enforces idempotency via stable IDs, confidence saturates at 1.0 without time decay (known MVP limitation), and strictly separates knowledge validation from permission changes (Learn≠Permission). Auto-approval is revocable via a flag defaulting to true.

## Related

- uses [[persistence-layer]] — Evolution artifacts (patterns, proposals, learned knowledge) stored via LibSQLStorage.
- uses [[runtime-kernel-observability]] — run_evolution drives the pipeline; kernel persists learned knowledge idempotently.
<!-- context:generated:end -->

## Notes

_Anything written below the generated block is preserved when the graph is regenerated._
