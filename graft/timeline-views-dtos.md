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
covers: []
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
