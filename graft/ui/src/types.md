# ui/src/types.ts · [[ui-dashboard]]

Single source of truth for all frontend API DTO types, mirroring the backend trace/evolution/memory schemas in camelCase.

- ViewKind · type · L3-L14 — Union of the five UI view modes (executive, operator, developer, memory, evolution) that drive which dashboard panel renders.
- AuthUserDto · interface · L17-L20 — interface AuthUserDto
- TraceEventDto · interface · L22-L34 — Wire shape of a single agent trace event as delivered by the trace API, carrying kind, event type, refs, reasoning, and I/O payloads.
- TimelineEvent · interface · L36-L45 — Flattened event shape used inside a run's timeline view, exposing the fields the timeline UI needs without run/agent context.
- DecisionSummary · interface · L47-L51 — Compact summary of a run's mission, decision, and risk for display in decision panels.
- RunTimeline · interface · L53-L61 — Aggregated view of a single run including status, progress, human-approval state, and its ordered timeline events.
- RunStartDto · interface · L63-L67 — Response payload returned when a run is started, providing the run id, status, and streaming URL for live updates.
- ApiResponse · interface · L69-L73 — Generic envelope wrapping every backend response with a code, message, and optional typed data payload.
- SkillDto · interface · L77-L85 — interface SkillDto
- PluginDto · interface · L87-L94 — interface PluginDto
- SessionEventDto · interface · L96-L101 — interface SessionEventDto
- WorkerDto · interface · L103-L111 — interface WorkerDto
- RawSkillDto · interface · L113-L119 — interface RawSkillDto
- SessionSummaryDto · interface · L121-L124 — interface SessionSummaryDto
- SessionDto · interface · L126-L129 — interface SessionDto
- WorldNodeDto · interface · L132-L136 — interface WorldNodeDto
- WorldEdgeDto · interface · L138-L144 — interface WorldEdgeDto
- WorldGraphDto · interface · L146-L149 — interface WorldGraphDto
