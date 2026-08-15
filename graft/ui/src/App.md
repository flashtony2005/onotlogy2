# ui/src/App.tsx · [[learn-permission-invariant]] [[ui-dashboard]]

- fmtTime · function · L33-L37 — Formats a timestamp string into a localized time string, falling back to the raw input if the date is invalid.
- fmtJson · function · L39-L47 — Pretty-prints arbitrary JSON values for display, returning strings as-is and falling back to String() on parse failure.
- App · function · L49-L273 — Root component orchestrating the agent observation dashboard: manages run state, streams trace events over WebSocket, builds the timeline, and switches between Executive/Operator/Developer/Memory/Evolution views.
- reloadMemory · function · L62-L72 — Fetches the latest memory records from the REST /memory endpoint and updates the Memory view state, preserving prior results on failure.
- reloadEvolution · function · L78-L88 — Fetches the aggregated evolution state from the REST /evolution endpoint for display-only visibility, preserving prior results on failure.
- startRun · function · L106-L123 — Validates the goal input, triggers a new agent run via postRun, resets the event stream, and preloads existing memory before the run starts.
- MemoryPanel · function · L276-L316 — Renders the memory sedimentation panel showing accumulated action-derived memory records with reuse hit counts and refresh control.
- EvolutionPanel · function · L319-L466 — Renders the self-learning knowledge panel exposing Pattern→Proposal→Learned evidence chain, auto-approve toggles, and approval audit, while enforcing Learn≠Permission (no Policy mutation).
- toggleAutoApprove · function · L337-L347 — Toggles a learned knowledge item's auto-approve eligibility via the REST endpoint (idempotent, revocable per REQ-018), then refreshes evolution state and approval audit.
- showAudit · function · L349-L352 — Fetches and displays the approval audit trail for auto-approve decisions.
