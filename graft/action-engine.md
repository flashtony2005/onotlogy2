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
covers: []
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
