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
  - symbol: COGNITIVE_STEPS
    kind: constant
    at: 'src/trace.rs:L12-L21'
  - symbol: View
    kind: enum
    at: 'src/trace.rs:L25-L32'
  - symbol: name
    kind: function
    at: 'src/trace.rs:L36-L42'
  - symbol: from_param
    kind: function
    at: 'src/trace.rs:L45-L51'
  - symbol: TraceEventDto
    kind: struct
    at: 'src/trace.rs:L57-L76'
  - symbol: from
    kind: function
    at: 'src/trace.rs:L79-L93'
  - symbol: TimelineEvent
    kind: struct
    at: 'src/trace.rs:L99-L114'
  - symbol: DecisionSummary
    kind: struct
    at: 'src/trace.rs:L118-L122'
  - symbol: RunTimeline
    kind: struct
    at: 'src/trace.rs:L127-L139'
  - symbol: derive_run_state
    kind: function
    at: 'src/trace.rs:L142-L159'
  - symbol: to_timeline_event
    kind: function
    at: 'src/trace.rs:L162-L173'
  - symbol: build_timeline
    kind: function
    at: 'src/trace.rs:L176-L248'
  - symbol: DecisionText
    kind: interface
    at: 'src/trace.rs:L251-L253'
  - symbol: description_or_output
    kind: function
    at: 'src/trace.rs:L252-L252'
  - symbol: description_or_output
    kind: function
    at: 'src/trace.rs:L255-L262'
  - symbol: tests
    kind: module
    at: 'src/trace.rs:L266-L300'
  - symbol: timeline_dto_is_camel_case
    kind: function
    at: 'src/trace.rs:L270-L299'
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
