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
covers:
  - symbol: TraceKind
    kind: enum
    at: 'src/runtime.rs:L19-L22'
  - symbol: as_str
    kind: function
    at: 'src/runtime.rs:L25-L30'
  - symbol: Err
    kind: type
    at: 'src/runtime.rs:L34-L34'
  - symbol: from_str
    kind: function
    at: 'src/runtime.rs:L35-L41'
  - symbol: TraceEvent
    kind: struct
    at: 'src/runtime.rs:L46-L59'
  - symbol: TRACE_SEQ
    kind: constant
    at: 'src/runtime.rs:L61-L61'
  - symbol: now_ts
    kind: function
    at: 'src/runtime.rs:L63-L68'
  - symbol: new_trace_id
    kind: function
    at: 'src/runtime.rs:L70-L76'
  - symbol: world
    kind: function
    at: 'src/runtime.rs:L80-L82'
  - symbol: world_with_refs
    kind: function
    at: 'src/runtime.rs:L86-L100'
  - symbol: EventBus
    kind: struct
    at: 'src/runtime.rs:L104-L106'
  - symbol: default
    kind: function
    at: 'src/runtime.rs:L109-L111'
  - symbol: new
    kind: function
    at: 'src/runtime.rs:L115-L118'
  - symbol: publish
    kind: function
    at: 'src/runtime.rs:L120-L122'
  - symbol: subscribe
    kind: function
    at: 'src/runtime.rs:L124-L126'
  - symbol: RuntimeKernel
    kind: struct
    at: 'src/runtime.rs:L131-L134'
  - symbol: new
    kind: function
    at: 'src/runtime.rs:L137-L142'
  - symbol: event_bus
    kind: function
    at: 'src/runtime.rs:L144-L146'
  - symbol: storage
    kind: function
    at: 'src/runtime.rs:L150-L152'
  - symbol: create_object
    kind: function
    at: 'src/runtime.rs:L154-L163'
  - symbol: update_object
    kind: function
    at: 'src/runtime.rs:L165-L170'
  - symbol: get_object
    kind: function
    at: 'src/runtime.rs:L173-L175'
  - symbol: delete_object
    kind: function
    at: 'src/runtime.rs:L177-L181'
  - symbol: emit
    kind: function
    at: 'src/runtime.rs:L184-L188'
  - symbol: emit_cognitive
    kind: function
    at: 'src/runtime.rs:L193-L218'
  - symbol: list_events
    kind: function
    at: 'src/runtime.rs:L221-L223'
  - symbol: get_trace
    kind: function
    at: 'src/runtime.rs:L226-L228'
  - symbol: list_world_events
    kind: function
    at: 'src/runtime.rs:L231-L233'
  - symbol: snapshot_world
    kind: function
    at: 'src/runtime.rs:L236-L248'
  - symbol: query_objects
    kind: function
    at: 'src/runtime.rs:L251-L256'
  - symbol: link_relation
    kind: function
    at: 'src/runtime.rs:L261-L277'
  - symbol: neighbors
    kind: function
    at: 'src/runtime.rs:L280-L282'
  - symbol: query_relations
    kind: function
    at: 'src/runtime.rs:L285-L290'
  - symbol: emit_domain_event
    kind: function
    at: 'src/runtime.rs:L293-L301'
  - symbol: record_memory
    kind: function
    at: 'src/runtime.rs:L306-L312'
  - symbol: recall_memory
    kind: function
    at: 'src/runtime.rs:L315-L320'
  - symbol: recall_all
    kind: function
    at: 'src/runtime.rs:L323-L325'
  - symbol: run_evolution
    kind: function
    at: 'src/runtime.rs:L334-L394'
  - symbol: evolution_state
    kind: function
    at: 'src/runtime.rs:L397-L403'
  - symbol: revoke_learned
    kind: function
    at: 'src/runtime.rs:L406-L408'
  - symbol: save_learned
    kind: function
    at: 'src/runtime.rs:L413-L418'
  - symbol: set_learned_auto_approve
    kind: function
    at: 'src/runtime.rs:L422-L428'
  - symbol: record_approval_audit
    kind: function
    at: 'src/runtime.rs:L431-L442'
  - symbol: list_approval_audit
    kind: function
    at: 'src/runtime.rs:L445-L450'
  - symbol: count_action_failed
    kind: function
    at: 'src/runtime.rs:L453-L455'
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
