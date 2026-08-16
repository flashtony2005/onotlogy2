# ui/src/types.ts · [[ui-dashboard]]

Single source of truth for all frontend API DTO types, mirroring the backend trace/evolution/memory schemas in camelCase.

- ViewKind · type · L3-L16 — Union of the five UI view modes (executive, operator, developer, memory, evolution) that drive which dashboard panel renders.
- AuthUserDto · interface · L19-L22 — interface AuthUserDto
- TraceEventDto · interface · L24-L36 — Wire shape of a single agent trace event as delivered by the trace API, carrying kind, event type, refs, reasoning, and I/O payloads.
- TimelineEvent · interface · L38-L47 — Flattened event shape used inside a run's timeline view, exposing the fields the timeline UI needs without run/agent context.
- DecisionSummary · interface · L49-L53 — Compact summary of a run's mission, decision, and risk for display in decision panels.
- RunTimeline · interface · L55-L63 — Aggregated view of a single run including status, progress, human-approval state, and its ordered timeline events.
- RunStartDto · interface · L65-L69 — Response payload returned when a run is started, providing the run id, status, and streaming URL for live updates.
- ApiResponse · interface · L71-L75 — Generic envelope wrapping every backend response with a code, message, and optional typed data payload.
- SkillDto · interface · L79-L87 — interface SkillDto
- PluginDto · interface · L89-L96 — interface PluginDto
- SessionEventDto · interface · L98-L103 — interface SessionEventDto
- WorkerDto · interface · L105-L113 — interface WorkerDto
- RawSkillDto · interface · L115-L121 — interface RawSkillDto
- SessionSummaryDto · interface · L123-L126 — interface SessionSummaryDto
- SessionDto · interface · L128-L131 — interface SessionDto
- WorldNodeDto · interface · L134-L138 — interface WorldNodeDto
- WorldEdgeDto · interface · L140-L146 — interface WorldEdgeDto
- WorldGraphDto · interface · L148-L151 — interface WorldGraphDto
- WorldRelationDto · interface · L156-L162 — interface WorldRelationDto
- WorldViewDto · interface · L164-L168 — interface WorldViewDto
- WorldQueryDto · interface · L170-L175 — interface WorldQueryDto
- AuditEventDto · interface · L177-L183 — interface AuditEventDto
- AuditViewDto · interface · L185-L188 — interface AuditViewDto
- AuditQueryDto · interface · L190-L196 — interface AuditQueryDto
- FabricRequestDto · interface · L198-L204 — interface FabricRequestDto
- FabricSubmissionDto · interface · L206-L221 — interface FabricSubmissionDto
- ProjectDto · interface · L225-L232 — interface ProjectDto
- ProjectCreateDto · interface · L234-L239 — interface ProjectCreateDto
