---
name: Persistence Layer
slug: persistence-layer
type: system
sources:
  - path: src/storage.rs
    hash: e814f6bb191298d5ddc7cbfabd21208a16f55ce5c5464b6aeb8662f2c1b43439
sources_digest: cdea642f9fff23d9935d4f45de249d1e58284cf06d7cae165477f54a5cc83a0d
links:
  - to: auth-rbac
    relation: uses
    description: >-
      LibSQLStorage backs wb_user and wb_session tables for identity and
      sessions.
  - to: code-generation
    relation: uses
    description: Uses canonical_schema DDL for table creation and version checks.
  - to: runtime-kernel-observability
    relation: uses
    description: Stores TraceEvent and evolution types defined in runtime.rs/evolution.rs.
generator:
  version: 1
covers: []
---
<!-- context:generated:start -->
## Summary

The libSQL/Turso-backed storage implementing the ObjectEngine trait. Handles CRUD for objects, relation edges, trace events, memory, and evolution artifacts, with schema versioning (WB2101 rejects stale DBs), atomic sequence counters for semantic IDs, and a schema-probe strategy using `SELECT id ... LIMIT 0` to avoid unstable PRAGMA queries. Enforces non-empty `object_refs` on trace events (WB2006) and merges `run_ids` across evolution runs.

## Related

- uses [[auth-rbac]] — LibSQLStorage backs wb_user and wb_session tables for identity and sessions.
- uses [[code-generation]] — Uses canonical_schema DDL for table creation and version checks.
- uses [[runtime-kernel-observability]] — Stores TraceEvent and evolution types defined in runtime.rs/evolution.rs.
<!-- context:generated:end -->

## Notes

_Anything written below the generated block is preserved when the graph is regenerated._
