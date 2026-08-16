---
name: Timeline Views & DTOs
slug: timeline-views-dtos
type: system
sources:
  - path: src/trace.rs
    hash: 9bf0f678764ca231eca4ffed0cb6a4405cbeef02a0883444ceee3397c224d79e
sources_digest: abc97c6450bb7bb661ae2a4e1a698b6381a30b41a164107c882584b49d32b691
links:
  - to: runtime-kernel-observability
    relation: uses
    description: Consumes TraceEvent data.
  - to: server-api
    relation: produces
    description: RunTimeline and TraceEventDto are the API/WS payloads served by server.rs.
generator:
  version: 1
covers:
  - symbol: TraceKind
    kind: enum
    at: 'src/trace.rs:L16-L19'
  - symbol: as_str
    kind: function
    at: 'src/trace.rs:L22-L27'
  - symbol: Err
    kind: type
    at: 'src/trace.rs:L31-L31'
  - symbol: from_str
    kind: function
    at: 'src/trace.rs:L32-L38'
  - symbol: TraceEvent
    kind: struct
    at: 'src/trace.rs:L43-L56'
  - symbol: TRACE_SEQ
    kind: constant
    at: 'src/trace.rs:L58-L58'
  - symbol: now_ts
    kind: function
    at: 'src/trace.rs:L61-L66'
  - symbol: new_trace_id
    kind: function
    at: 'src/trace.rs:L69-L75'
  - symbol: world
    kind: function
    at: 'src/trace.rs:L79-L81'
  - symbol: world_with_refs
    kind: function
    at: 'src/trace.rs:L85-L99'
  - symbol: COGNITIVE_STEPS
    kind: constant
    at: 'src/trace.rs:L103-L112'
  - symbol: View
    kind: enum
    at: 'src/trace.rs:L116-L123'
  - symbol: name
    kind: function
    at: 'src/trace.rs:L127-L133'
  - symbol: from_param
    kind: function
    at: 'src/trace.rs:L136-L142'
  - symbol: TraceEventDto
    kind: struct
    at: 'src/trace.rs:L148-L167'
  - symbol: from
    kind: function
    at: 'src/trace.rs:L170-L184'
  - symbol: from_view
    kind: function
    at: 'src/trace.rs:L193-L211'
  - symbol: TimelineEvent
    kind: struct
    at: 'src/trace.rs:L217-L232'
  - symbol: DecisionSummary
    kind: struct
    at: 'src/trace.rs:L236-L240'
  - symbol: RunTimeline
    kind: struct
    at: 'src/trace.rs:L245-L257'
  - symbol: derive_run_state
    kind: function
    at: 'src/trace.rs:L262-L288'
  - symbol: to_timeline_event
    kind: function
    at: 'src/trace.rs:L291-L302'
  - symbol: build_timeline
    kind: function
    at: 'src/trace.rs:L305-L384'
  - symbol: DecisionText
    kind: interface
    at: 'src/trace.rs:L387-L389'
  - symbol: description_or_output
    kind: function
    at: 'src/trace.rs:L388-L388'
  - symbol: description_or_output
    kind: function
    at: 'src/trace.rs:L391-L398'
  - symbol: tests
    kind: module
    at: 'src/trace.rs:L402-L491'
  - symbol: timeline_dto_is_camel_case
    kind: function
    at: 'src/trace.rs:L406-L435'
  - symbol: ws_from_view_clamps_to_role
    kind: function
    at: 'src/trace.rs:L439-L490'
---
<!-- context:generated:start -->
## Summary

The three-tier observation layer (Executive/Operator/Developer) over a single TraceEvent source. `build_timeline` filters per role: Executive keeps Goal/Action + DecisionSummary, Operator dedupes to first occurrence of each of 8 COGNITIVE_STEPS, Developer exposes full reasoning/IO. `derive_run_state` computes progress, status (Completed on MemoryUpdate), and approval requirement (Required on PolicyCheck 'Deny'). Server-side view clamping means the returned view reflects the enforced role, not the client request.

## Related

- uses [[runtime-kernel-observability]] — Consumes TraceEvent data.
- produces [[server-api]] — RunTimeline and TraceEventDto are the API/WS payloads served by server.rs.
<!-- context:generated:end -->

## Notes

_Anything written below the generated block is preserved when the graph is regenerated._
