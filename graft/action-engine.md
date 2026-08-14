---
name: Action Engine
slug: action-engine
type: system
sources:
  - path: src/action.rs
    hash: 64d2dce7c3f6d1748ef7c756476661d8cf31ff8a60324ddeb38069754cfeac6a
sources_digest: d527ba8278c7c859a6505daaabab107d4314c76e4814ab1e234123548cbdb1db
links:
  - to: ontology-ir-odl-parsing
    relation: uses
    description: ActionRegistry maps ActionDef names from OntologyIR.
  - to: planner-policy-engine
    relation: uses
    description: >-
      Planner invokes ActionEngine for plan steps; policies gate which actions
      run.
  - to: runtime-kernel-observability
    relation: uses
    description: Executes world-model operations through RuntimeKernel.
generator:
  version: 1
covers:
  - symbol: ActionRegistry
    kind: struct
    at: 'src/action.rs:L20-L22'
  - symbol: empty
    kind: function
    at: 'src/action.rs:L26-L28'
  - symbol: from_ir
    kind: function
    at: 'src/action.rs:L31-L41'
  - symbol: get
    kind: function
    at: 'src/action.rs:L44-L46'
  - symbol: actions_of
    kind: function
    at: 'src/action.rs:L49-L54'
  - symbol: has_actions
    kind: function
    at: 'src/action.rs:L57-L59'
  - symbol: is_empty
    kind: function
    at: 'src/action.rs:L62-L64'
  - symbol: ActionError
    kind: enum
    at: 'src/action.rs:L69-L78'
  - symbol: ActionEffect
    kind: struct
    at: 'src/action.rs:L82-L89'
  - symbol: ActionEngine
    kind: struct
    at: 'src/action.rs:L92-L94'
  - symbol: new
    kind: function
    at: 'src/action.rs:L97-L99'
  - symbol: invoke
    kind: function
    at: 'src/action.rs:L103-L182'
  - symbol: resolve
    kind: function
    at: 'src/action.rs:L186-L193'
---
<!-- context:generated:start -->
## Summary

The declarative action interpreter replacing V0.3 hardcoded logic. Builds a type-to-action map from OntologyIR, executes idempotent updates (upserts), creates, and links (deduplicated edges), emitting `action.<name>` domain events. Requires the target object to exist and re-reads objects between updates to avoid field overwrites. Falls back to legacy planner behavior when the registry is empty.

## Related

- uses [[ontology-ir-odl-parsing]] — ActionRegistry maps ActionDef names from OntologyIR.
- uses [[planner-policy-engine]] — Planner invokes ActionEngine for plan steps; policies gate which actions run.
- uses [[runtime-kernel-observability]] — Executes world-model operations through RuntimeKernel.
<!-- context:generated:end -->

## Notes

_Anything written below the generated block is preserved when the graph is regenerated._
