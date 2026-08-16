---
name: Runtime Kernel & Observability
slug: runtime-kernel-observability
type: system
sources:
  - path: src/runtime.rs
    hash: 5f2cab0b89c4304f864d96ccf1b2be11dc741f2d472d3eae69974bfe62cec174
sources_digest: 78414edc9f1dff05e9d55df046e41b0b1c06449f84d8948edc33088ef44a7343
links:
  - to: action-engine
    relation: uses
    description: ActionEngine executes updates/creates/links through RuntimeKernel methods.
  - to: evolution-engine
    relation: uses
    description: >-
      run_evolution mines patterns from memories, validates proposals, persists
      learned knowledge.
  - to: persistence-layer
    relation: uses
    description: Wraps LibSQLStorage for all persistence operations.
generator:
  version: 1
covers: []
---
<!-- context:generated:start -->
## Summary

The central orchestrator wrapping LibSQLStorage and EventBus. Every object CRUD persists, emits a world TraceEvent, appends to the trace log, and publishes to the bus in a single `emit` flow (write-then-publish, TC-008). TraceEvent unifies world and cognitive events with mandatory object_refs for explainability; EventBus is a 1024-capacity non-blocking broadcast that drops events when receivers are full. Also drives the evolution pipeline and REQ-018 auto-approval gating.

## Related

- uses [[action-engine]] — ActionEngine executes updates/creates/links through RuntimeKernel methods.
- uses [[evolution-engine]] — run_evolution mines patterns from memories, validates proposals, persists learned knowledge.
- uses [[persistence-layer]] — Wraps LibSQLStorage for all persistence operations.
<!-- context:generated:end -->

## Notes

_Anything written below the generated block is preserved when the graph is regenerated._
