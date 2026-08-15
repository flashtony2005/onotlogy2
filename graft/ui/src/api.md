# ui/src/api.ts · [[learn-permission-invariant]] [[ui-dashboard]]

REST client module for the agent backend, wrapping all API calls in the {code,msg,data} envelope with /api/v1/wb/agent prefix.

- unwrap · function · L13-L19 — Unwraps the backend envelope, throwing on non-zero code or missing data so callers get typed payloads or a clear error.
- postRun · function · L21-L28 — Starts a new agent run by POSTing the agent id and goal to the run endpoint.
- getTimeline · function · L30-L35 — Fetches the run timeline for a given run id and view, URL-encoding the run id.
- getMemory · function · L38-L41 — Fetches the full memory store for the V0.5 Memory view.
- getEvolution · function · L45-L48 — Fetches the Evolution aggregate state, exposing only knowledge without any policy-rewrite or approval-bypass capability.
- postAutoApproveToggle · function · L52-L62 — Toggles (idempotently, reversibly) whether a learned knowledge auto-approves its action or falls back to Human Approval.
- getApprovalAudit · function · L65-L68 — Lists the auto-approval audit trail traceable to learned knowledge and runs.
