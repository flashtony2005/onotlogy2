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
covers:
  - symbol: REL_SEQ
    kind: constant
    at: 'src/storage.rs:L17-L17'
  - symbol: TRACE_SEQ_AUDIT
    kind: constant
    at: 'src/storage.rs:L20-L20'
  - symbol: ApprovalAudit
    kind: struct
    at: 'src/storage.rs:L24-L33'
  - symbol: UserRecord
    kind: struct
    at: 'src/storage.rs:L36-L43'
  - symbol: SessionRecord
    kind: struct
    at: 'src/storage.rs:L46-L51'
  - symbol: now_ts
    kind: function
    at: 'src/storage.rs:L53-L58'
  - symbol: ObjectRecord
    kind: struct
    at: 'src/storage.rs:L62-L67'
  - symbol: RelationRecord
    kind: struct
    at: 'src/storage.rs:L71-L77'
  - symbol: MemoryRecord
    kind: struct
    at: 'src/storage.rs:L82-L88'
  - symbol: StorageError
    kind: enum
    at: 'src/storage.rs:L92-L101'
  - symbol: TraceQuery
    kind: struct
    at: 'src/storage.rs:L106-L113'
  - symbol: for_run
    kind: function
    at: 'src/storage.rs:L117-L123'
  - symbol: for_agent
    kind: function
    at: 'src/storage.rs:L125-L131'
  - symbol: with_after
    kind: function
    at: 'src/storage.rs:L133-L136'
  - symbol: with_object
    kind: function
    at: 'src/storage.rs:L138-L141'
  - symbol: ObjectEngine
    kind: interface
    at: 'src/storage.rs:L146-L157'
  - symbol: create
    kind: function
    at: 'src/storage.rs:L147-L152'
  - symbol: get
    kind: function
    at: 'src/storage.rs:L153-L153'
  - symbol: update
    kind: function
    at: 'src/storage.rs:L154-L154'
  - symbol: query
    kind: function
    at: 'src/storage.rs:L155-L155'
  - symbol: delete
    kind: function
    at: 'src/storage.rs:L156-L156'
  - symbol: RelationEngine
    kind: interface
    at: 'src/storage.rs:L161-L181'
  - symbol: link
    kind: function
    at: 'src/storage.rs:L163-L169'
  - symbol: neighbors
    kind: function
    at: 'src/storage.rs:L171-L171'
  - symbol: query_by_edge
    kind: function
    at: 'src/storage.rs:L173-L173'
  - symbol: unlink
    kind: function
    at: 'src/storage.rs:L175-L180'
  - symbol: LibSQLStorage
    kind: struct
    at: 'src/storage.rs:L187-L189'
  - symbol: SCHEMA_VERSION
    kind: constant
    at: 'src/storage.rs:L192-L192'
  - symbol: is_no_such_table
    kind: function
    at: 'src/storage.rs:L195-L197'
  - symbol: open
    kind: function
    at: 'src/storage.rs:L201-L206'
  - symbol: ensure_schema
    kind: function
    at: 'src/storage.rs:L217-L274'
  - symbol: from_connection
    kind: function
    at: 'src/storage.rs:L277-L279'
  - symbol: append_trace
    kind: function
    at: 'src/storage.rs:L283-L314'
  - symbol: list_events
    kind: function
    at: 'src/storage.rs:L318-L372'
  - symbol: get_trace
    kind: function
    at: 'src/storage.rs:L375-L389'
  - symbol: list_world_events
    kind: function
    at: 'src/storage.rs:L392-L406'
  - symbol: list_all_objects
    kind: function
    at: 'src/storage.rs:L409-L422'
  - symbol: list_all_relations
    kind: function
    at: 'src/storage.rs:L425-L438'
  - symbol: render_object_list
    kind: function
    at: 'src/storage.rs:L441-L449'
  - symbol: record_memory
    kind: function
    at: 'src/storage.rs:L454-L472'
  - symbol: recall_memory
    kind: function
    at: 'src/storage.rs:L475-L487'
  - symbol: recall_all
    kind: function
    at: 'src/storage.rs:L490-L504'
  - symbol: save_pattern
    kind: function
    at: 'src/storage.rs:L509-L537'
  - symbol: read_pattern_run_ids
    kind: function
    at: 'src/storage.rs:L539-L551'
  - symbol: read_proposal_run_ids
    kind: function
    at: 'src/storage.rs:L553-L562'
  - symbol: read_learned_run_ids
    kind: function
    at: 'src/storage.rs:L564-L573'
  - symbol: list_patterns
    kind: function
    at: 'src/storage.rs:L576-L590'
  - symbol: save_proposal
    kind: function
    at: 'src/storage.rs:L593-L631'
  - symbol: list_proposals
    kind: function
    at: 'src/storage.rs:L634-L648'
  - symbol: save_learned
    kind: function
    at: 'src/storage.rs:L651-L684'
  - symbol: list_learned
    kind: function
    at: 'src/storage.rs:L687-L703'
  - symbol: delete_learned
    kind: function
    at: 'src/storage.rs:L706-L711'
  - symbol: set_learned_auto_approve
    kind: function
    at: 'src/storage.rs:L715-L727'
  - symbol: record_approval_audit
    kind: function
    at: 'src/storage.rs:L730-L751'
  - symbol: list_approval_audit
    kind: function
    at: 'src/storage.rs:L754-L792'
  - symbol: count_action_failed
    kind: function
    at: 'src/storage.rs:L796-L810'
  - symbol: row_to_pattern
    kind: function
    at: 'src/storage.rs:L813-L822'
  - symbol: row_to_proposal
    kind: function
    at: 'src/storage.rs:L824-L850'
  - symbol: row_to_learned
    kind: function
    at: 'src/storage.rs:L852-L876'
  - symbol: create
    kind: function
    at: 'src/storage.rs:L880-L899'
  - symbol: get
    kind: function
    at: 'src/storage.rs:L901-L914'
  - symbol: update
    kind: function
    at: 'src/storage.rs:L916-L943'
  - symbol: query
    kind: function
    at: 'src/storage.rs:L945-L958'
  - symbol: delete
    kind: function
    at: 'src/storage.rs:L960-L972'
  - symbol: hard_delete
    kind: function
    at: 'src/storage.rs:L977-L983'
  - symbol: RunRecord
    kind: struct
    at: 'src/storage.rs:L989-L998'
  - symbol: ApprovalRecord
    kind: struct
    at: 'src/storage.rs:L1001-L1012'
  - symbol: insert_run
    kind: function
    at: 'src/storage.rs:L1016-L1029'
  - symbol: update_run_status
    kind: function
    at: 'src/storage.rs:L1032-L1049'
  - symbol: list_runs
    kind: function
    at: 'src/storage.rs:L1052-L1066'
  - symbol: get_run
    kind: function
    at: 'src/storage.rs:L1069-L1082'
  - symbol: insert_approval
    kind: function
    at: 'src/storage.rs:L1085-L1101'
  - symbol: update_approval_state
    kind: function
    at: 'src/storage.rs:L1104-L1126'
  - symbol: list_approvals_for_run
    kind: function
    at: 'src/storage.rs:L1129-L1146'
  - symbol: list_approvals_by_state
    kind: function
    at: 'src/storage.rs:L1149-L1166'
  - symbol: recover_persisted_state
    kind: function
    at: 'src/storage.rs:L1170-L1184'
  - symbol: row_to_run
    kind: function
    at: 'src/storage.rs:L1187-L1199'
  - symbol: row_to_approval
    kind: function
    at: 'src/storage.rs:L1201-L1216'
  - symbol: link
    kind: function
    at: 'src/storage.rs:L1220-L1260'
  - symbol: neighbors
    kind: function
    at: 'src/storage.rs:L1262-L1275'
  - symbol: query_by_edge
    kind: function
    at: 'src/storage.rs:L1277-L1290'
  - symbol: unlink
    kind: function
    at: 'src/storage.rs:L1292-L1306'
  - symbol: row_to_object
    kind: function
    at: 'src/storage.rs:L1309-L1317'
  - symbol: row_to_relation
    kind: function
    at: 'src/storage.rs:L1319-L1328'
  - symbol: row_to_memory
    kind: function
    at: 'src/storage.rs:L1330-L1339'
  - symbol: row_to_trace
    kind: function
    at: 'src/storage.rs:L1341-L1364'
  - symbol: create_user
    kind: function
    at: 'src/storage.rs:L1369-L1384'
  - symbol: get_user_by_username
    kind: function
    at: 'src/storage.rs:L1387-L1402'
  - symbol: count_users
    kind: function
    at: 'src/storage.rs:L1405-L1414'
  - symbol: get_user_by_id
    kind: function
    at: 'src/storage.rs:L1417-L1429'
  - symbol: set_user_disabled
    kind: function
    at: 'src/storage.rs:L1432-L1447'
  - symbol: save_session
    kind: function
    at: 'src/storage.rs:L1450-L1463'
  - symbol: get_session
    kind: function
    at: 'src/storage.rs:L1466-L1483'
  - symbol: delete_session
    kind: function
    at: 'src/storage.rs:L1486-L1491'
  - symbol: delete_sessions_for_user
    kind: function
    at: 'src/storage.rs:L1494-L1502'
  - symbol: user_from_row
    kind: function
    at: 'src/storage.rs:L1506-L1515'
  - symbol: tests
    kind: module
    at: 'src/storage.rs:L1518-L1556'
  - symbol: ensure_schema_rejects_legacy
    kind: function
    at: 'src/storage.rs:L1523-L1542'
  - symbol: open_creates_fresh_db_with_version
    kind: function
    at: 'src/storage.rs:L1546-L1555'
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
