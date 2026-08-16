# ui/src/types.ts · [[ui-dashboard]]

Single source of truth for all frontend API DTO types, mirroring the backend trace/evolution/memory schemas in camelCase.

- ViewKind · type · L3-L15 — Union of the five UI view modes (executive, operator, developer, memory, evolution) that drive which dashboard panel renders.
- AuthUserDto · interface · L18-L21 — interface AuthUserDto
- TraceEventDto · interface · L23-L35 — Wire shape of a single agent trace event as delivered by the trace API, carrying kind, event type, refs, reasoning, and I/O payloads.
- TimelineEvent · interface · L37-L46 — Flattened event shape used inside a run's timeline view, exposing the fields the timeline UI needs without run/agent context.
- DecisionSummary · interface · L48-L52 — Compact summary of a run's mission, decision, and risk for display in decision panels.
- RunTimeline · interface · L54-L62 — Aggregated view of a single run including status, progress, human-approval state, and its ordered timeline events.
- RunStartDto · interface · L64-L68 — Response payload returned when a run is started, providing the run id, status, and streaming URL for live updates.
- ApiResponse · interface · L70-L74 — Generic envelope wrapping every backend response with a code, message, and optional typed data payload.
- SkillDto · interface · L78-L86 — interface SkillDto
- PluginDto · interface · L88-L95 — interface PluginDto
- SessionEventDto · interface · L97-L102 — interface SessionEventDto
- WorkerDto · interface · L104-L112 — interface WorkerDto
- RawSkillDto · interface · L114-L120 — interface RawSkillDto
- SessionSummaryDto · interface · L122-L125 — interface SessionSummaryDto
- SessionDto · interface · L127-L130 — interface SessionDto
- WorldNodeDto · interface · L133-L137 — interface WorldNodeDto
- WorldEdgeDto · interface · L139-L145 — interface WorldEdgeDto
- WorldGraphDto · interface · L147-L150 — interface WorldGraphDto
- WorldRelationDto · interface · L155-L161 — interface WorldRelationDto
- WorldViewDto · interface · L163-L167 — interface WorldViewDto
- WorldQueryDto · interface · L169-L173 — interface WorldQueryDto
- AuditEventDto · interface · L175-L181 — interface AuditEventDto
- AuditViewDto · interface · L183-L186 — interface AuditViewDto
- AuditQueryDto · interface · L188-L193 — interface AuditQueryDto
- FabricRequestDto · interface · L195-L200 — interface FabricRequestDto
- FabricSubmissionDto · interface · L202-L217 — interface FabricSubmissionDto
