# src/trace.rs · [[server-side-view-clamping]] [[timeline-views-dtos]]

- COGNITIVE_STEPS · constant · L12-L21 — Defines the canonical ordered list of 8 cognitive step types used for progress calculation and Operator-view pipeline filtering.
- View · enum · L25-L32 — Enumerates the three role-based timeline granularities (Executive/Operator/Developer) that determine which event details are exposed.
- name · function · L36-L42 — Maps a View variant to its lowercase string name used in response fields and server-side clamping assertions.
- from_param · function · L45-L51 — Parses a query parameter string into a View, defaulting to Operator for unknown or absent values.
- TraceEventDto · struct · L57-L76 — CamelCase serialization DTO for TraceEvent used as the unified API/WebSocket payload, skipping empty optional fields.
- from · function · L79-L93 — Converts a TraceEvent into its camelCase DTO form by copying all fields verbatim.
- TimelineEvent · struct · L99-L114 — A single view-cropped timeline event carrying only the fields permitted by the active view, with reasoning/IO only in Developer view.
- DecisionSummary · struct · L118-L122 — Executive-view decision digest holding mission, decision text, and computed risk score.
- RunTimeline · struct · L127-L139 — Unified run timeline response structure combining status, progress, approval requirement, clamped view, events, and optional Executive decision summary.
- derive_run_state · function · L142-L159 — Derives run status, progress percentage, and human-approval requirement from the set of events by counting cognitive steps, detecting MemoryUpdate completion, and flagging PolicyCheck Deny outputs.
- to_timeline_event · function · L162-L173 — Builds a TimelineEvent from a TraceEvent, including reasoning/IO fields only when detail (Developer view) is requested.
- build_timeline · function · L176-L248 — Builds the run timeline and crops events per view: Executive keeps only Goal/Action plus a decision summary, Operator shows first occurrence of each cognitive step, Developer exposes full detail.
- DecisionText · interface · L251-L253 — Trait declaring a method to extract a human-readable decision description from a TraceEvent.
- description_or_output · function · L252-L252 — Trait method signature for extracting a decision description from a TraceEvent.
- description_or_output · function · L255-L262 — Extracts an Action's decision text by preferring string output, falling back to reasoning, then event type.
- tests · module · L266-L300 — Unit test module for the trace layer.
- timeline_dto_is_camel_case · function · L270-L299 — Verifies that RunTimeline and TimelineEvent serialize with camelCase field names and no snake_case keys.
