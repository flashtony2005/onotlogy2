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
    at: 'src/trace.rs:L15-L18'
  - symbol: as_str
    kind: function
    at: 'src/trace.rs:L21-L26'
  - symbol: Err
    kind: type
    at: 'src/trace.rs:L30-L30'
  - symbol: from_str
    kind: function
    at: 'src/trace.rs:L31-L37'
  - symbol: TraceEvent
    kind: struct
    at: 'src/trace.rs:L42-L55'
  - symbol: TRACE_SEQ
    kind: constant
    at: 'src/trace.rs:L57-L57'
  - symbol: now_ts
    kind: function
    at: 'src/trace.rs:L60-L65'
  - symbol: new_trace_id
    kind: function
    at: 'src/trace.rs:L68-L74'
  - symbol: world
    kind: function
    at: 'src/trace.rs:L78-L80'
  - symbol: world_with_refs
    kind: function
    at: 'src/trace.rs:L84-L98'
  - symbol: COGNITIVE_STEPS
    kind: constant
    at: 'src/trace.rs:L102-L111'
  - symbol: View
    kind: enum
    at: 'src/trace.rs:L115-L122'
  - symbol: name
    kind: function
    at: 'src/trace.rs:L126-L132'
  - symbol: from_param
    kind: function
    at: 'src/trace.rs:L135-L141'
  - symbol: TraceEventDto
    kind: struct
    at: 'src/trace.rs:L147-L166'
  - symbol: from
    kind: function
    at: 'src/trace.rs:L169-L183'
  - symbol: from_view
    kind: function
    at: 'src/trace.rs:L192-L210'
  - symbol: TimelineEvent
    kind: struct
    at: 'src/trace.rs:L216-L231'
  - symbol: DecisionSummary
    kind: struct
    at: 'src/trace.rs:L235-L239'
  - symbol: RunTimeline
    kind: struct
    at: 'src/trace.rs:L244-L256'
  - symbol: derive_run_state
    kind: function
    at: 'src/trace.rs:L261-L287'
  - symbol: to_timeline_event
    kind: function
    at: 'src/trace.rs:L290-L301'
  - symbol: build_timeline
    kind: function
    at: 'src/trace.rs:L304-L383'
  - symbol: DecisionText
    kind: interface
    at: 'src/trace.rs:L386-L388'
  - symbol: description_or_output
    kind: function
    at: 'src/trace.rs:L387-L387'
  - symbol: description_or_output
    kind: function
    at: 'src/trace.rs:L390-L397'
  - symbol: tests
    kind: module
    at: 'src/trace.rs:L401-L490'
  - symbol: timeline_dto_is_camel_case
    kind: function
    at: 'src/trace.rs:L405-L434'
  - symbol: ws_from_view_clamps_to_role
    kind: function
    at: 'src/trace.rs:L438-L489'
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
