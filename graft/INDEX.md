# graft — repo map

Small markdown nodes summarising this repo. `grep` any term, symbol, or
filename here, or run `graft ask "<task>"`. Each node carries prose plus exact
`file:line`; open a source file only to edit the named span.

The same graph is queryable as MCP tools (`graft_find_code`, `graft_find_all`,
`graft_trace_calls`, `graft_file_api`, `graft_repo_map`) where a host exposes them, and
as the `graft` CLI everywhere else. Edges — who calls what — live only in the
graph, not in these files: `graft callers <symbol>` is the only way to read them.

## Concepts

- [action-engine](action-engine.md) — Action Engine · src/action.rs
- [auth-persistence-integration-tests](auth-persistence-integration-tests.md) — Auth & Persistence Integration Tests · tests/v0_7_auth.rs, tests/v0_8_persistence.rs
- [auth-rbac](auth-rbac.md) — Auth & RBAC · src/auth.rs
- [cli-entry-point](cli-entry-point.md) — CLI Entry Point · src/bin/oaip.rs
- [code-generation](code-generation.md) — Code Generation · src/codegen.rs
- [cognitive-engine-pi](cognitive-engine-pi.md) — Cognitive Engine (Pi) · src/agent.rs
- [evolution-engine](evolution-engine.md) — Evolution Engine · src/evolution.rs
- [idempotency-stable-ids](idempotency-stable-ids.md) — Idempotency & Stable IDs · src/action.rs, src/evolution.rs, src/storage.rs, tests/v0_3.rs
- [learn-permission-invariant](learn-permission-invariant.md) — Learn≠Permission Invariant · src/evolution.rs, src/planner.rs, src/runtime.rs, ui/src/api.ts, ui/src/App.tsx
- [ontology-ir-odl-parsing](ontology-ir-odl-parsing.md) — Ontology IR & ODL Parsing · src/ir.rs, src/odl.rs
- [persistence-layer](persistence-layer.md) — Persistence Layer · src/storage.rs
- [planner-policy-engine](planner-policy-engine.md) — Planner & Policy Engine · src/planner.rs
- [runtime-kernel-observability](runtime-kernel-observability.md) — Runtime Kernel & Observability · src/runtime.rs
- [server-api](server-api.md) — Server & API · src/server.rs
- [server-side-view-clamping](server-side-view-clamping.md) — Server-Side View Clamping · src/auth.rs, src/server.rs, src/trace.rs, ui/src/view.ts
- [timeline-views-dtos](timeline-views-dtos.md) — Timeline Views & DTOs · src/trace.rs
- [ui-dashboard](ui-dashboard.md) — UI Dashboard · ui/src/api.ts, ui/src/App.tsx, ui/src/hooks/useTraceStream.ts, ui/src/main.tsx, ui/src/types.ts, ui/src/view.ts, ui/vite.config.ts
- [v0-1-v0-3-integration-contract](v0-1-v0-3-integration-contract.md) — V0.1–V0.3 Integration Contract · tests/v0_1.rs, tests/v0_2.rs, tests/v0_3.rs

## Files

182 per-file wiring cards mirror the source tree under `graft/` (179 carry extracted symbols). They are deliberately not enumerated here —
`grep` a symbol or `find`/`ls` a filename under `graft/` to land on the card for that file.
