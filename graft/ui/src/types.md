# ui/src/types.ts · [[ui-dashboard]]

Single source of truth for all frontend API DTO types, mirroring the backend trace/evolution/memory schemas in camelCase.

- ViewKind · type · L3-L3 — Union of the five UI view modes (executive, operator, developer, memory, evolution) that drive which dashboard panel renders.
- TraceEventDto · interface · L5-L17 — Wire shape of a single agent trace event as delivered by the trace API, carrying kind, event type, refs, reasoning, and I/O payloads.
- TimelineEvent · interface · L19-L28 — Flattened event shape used inside a run's timeline view, exposing the fields the timeline UI needs without run/agent context.
- DecisionSummary · interface · L30-L34 — Compact summary of a run's mission, decision, and risk for display in decision panels.
- RunTimeline · interface · L36-L44 — Aggregated view of a single run including status, progress, human-approval state, and its ordered timeline events.
- RunStartDto · interface · L46-L50 — Response payload returned when a run is started, providing the run id, status, and streaming URL for live updates.
- ApiResponse · interface · L52-L56 — Generic envelope wrapping every backend response with a code, message, and optional typed data payload.
- MemoryRecord · interface · L59-L65 — Shape of a single memory entry from the wb_memory backend, carrying label, payload, hit count, and timestamps.
- MemoryListDto · interface · L67-L70 — Paged listing of memory records with a total count alongside the array.
- PatternDto · interface · L74-L80 — An evolution pattern mined from runs, with support/confidence metrics and the run ids that produced it.
- ProposalDto · interface · L82-L90 — A proposed evolution change with a lifecycle status (Proposed/Validated/Rejected) linked back to its source pattern.
- LearnedDto · interface · L92-L102 — A validated evolution action that can be applied, optionally eligible for REQ-018 auto-approval which is revocable.
- ApprovalAuditDto · interface · L105-L113 — Audit record for REQ-018 auto-approval decisions, capturing whether an action was auto-approved, enabled, or had its approval revoked.
- EvolutionStateDto · interface · L115-L119 — Aggregate snapshot of the full evolution pipeline: patterns, proposals, and learned actions.
