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
covers:
  - symbol: PATTERN_MIN_HITS
    kind: constant
    at: 'src/evolution.rs:L27-L27'
  - symbol: CONFIDENCE_DIVISOR
    kind: constant
    at: 'src/evolution.rs:L29-L29'
  - symbol: VALIDATE_THRESHOLD
    kind: constant
    at: 'src/evolution.rs:L31-L31'
  - symbol: pattern_label_of
    kind: function
    at: 'src/evolution.rs:L35-L37'
  - symbol: proposal_id_of
    kind: function
    at: 'src/evolution.rs:L40-L42'
  - symbol: learned_id_of
    kind: function
    at: 'src/evolution.rs:L45-L47'
  - symbol: Experience
    kind: struct
    at: 'src/evolution.rs:L52-L55'
  - symbol: action
    kind: function
    at: 'src/evolution.rs:L59-L64'
  - symbol: Pattern
    kind: struct
    at: 'src/evolution.rs:L69-L80'
  - symbol: confidence_of
    kind: function
    at: 'src/evolution.rs:L87-L89'
  - symbol: ProposalKind
    kind: enum
    at: 'src/evolution.rs:L94-L96'
  - symbol: ProposalStatus
    kind: enum
    at: 'src/evolution.rs:L100-L104'
  - symbol: Proposal
    kind: struct
    at: 'src/evolution.rs:L109-L121'
  - symbol: LearnedKnowledge
    kind: struct
    at: 'src/evolution.rs:L126-L141'
  - symbol: default_true
    kind: function
    at: 'src/evolution.rs:L143-L145'
  - symbol: EvolutionState
    kind: struct
    at: 'src/evolution.rs:L149-L153'
  - symbol: mine_patterns
    kind: function
    at: 'src/evolution.rs:L159-L179'
  - symbol: propose
    kind: function
    at: 'src/evolution.rs:L182-L192'
  - symbol: validate
    kind: function
    at: 'src/evolution.rs:L199-L209'
  - symbol: parse_run_ids
    kind: function
    at: 'src/evolution.rs:L212-L214'
  - symbol: encode_run_ids
    kind: function
    at: 'src/evolution.rs:L217-L224'
  - symbol: merge_run_ids
    kind: function
    at: 'src/evolution.rs:L227-L234'
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
