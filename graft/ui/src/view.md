# ui/src/view.ts · [[server-side-view-clamping]] [[ui-dashboard]]

Three-tier timeline view cropping that mirrors the backend build_timeline so WS realtime streams and offline REST queries stay consistent across Executive/Operator/Developer views.

- toTimelineEvent · function · L25-L36 — Maps a trace event DTO to a timeline event, conditionally including reasoning/input/output only when detail is requested.
- asString · function · L38-L41 — Normalizes an unknown value to a string, JSON-stringifying non-string values and returning undefined for nullish input.
- deriveState · function · L45-L55 — function deriveState(events: TraceEventDto[])
- buildTimeline · function · L57-L110 — Builds a run timeline summary with status, progress, and human-approval flags, then crops events per view kind (memory/executive/operator/developer).
