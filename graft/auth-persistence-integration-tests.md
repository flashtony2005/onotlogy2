---
name: Auth & Persistence Integration Tests
slug: auth-persistence-integration-tests
type: system
sources:
  - path: tests/v0_7_auth.rs
    hash: 36f3a9cb8f9fbb0a22475daf5fd9b2300671457ef35397a8b159e892ec63c80c
  - path: tests/v0_8_persistence.rs
    hash: 5495f4360af933cd96f9565c37de67c5c2af0085df7972fa1d96bb4bbfe40459
sources_digest: f85cb0319f4839c27316e37f7fd4681b0a975c961b8c8801ff11d371a4b8bf1a
links:
  - to: auth-rbac
    relation: validates
    description: 'Exercises login/logout, role gating, view clamping, session invalidation.'
  - to: persistence-layer
    relation: validates
    description: Verifies run/approval state survives restarts via file-based DB.
  - to: server-api
    relation: validates
    description: Spawns real HTTP servers and exercises the full runtime path.
generator:
  version: 1
covers:
  - symbol: NoopEngine
    kind: struct
    at: 'tests/v0_7_auth.rs:L21-L21'
  - symbol: name
    kind: function
    at: 'tests/v0_7_auth.rs:L25-L27'
  - symbol: reason
    kind: function
    at: 'tests/v0_7_auth.rs:L28-L30'
  - symbol: http_call
    kind: function
    at: 'tests/v0_7_auth.rs:L34-L75'
  - symbol: login
    kind: function
    at: 'tests/v0_7_auth.rs:L78-L87'
  - symbol: spawn_auth_server
    kind: function
    at: 'tests/v0_7_auth.rs:L91-L128'
  - symbol: tc_auth_a_no_token_401
    kind: function
    at: 'tests/v0_7_auth.rs:L132-L143'
  - symbol: tc_auth_b_wrong_token_401
    kind: function
    at: 'tests/v0_7_auth.rs:L147-L158'
  - symbol: tc_auth_c_executive_run_forbidden
    kind: function
    at: 'tests/v0_7_auth.rs:L162-L175'
  - symbol: tc_auth_d_view_clamped_to_role
    kind: function
    at: 'tests/v0_7_auth.rs:L179-L200'
  - symbol: tc_auth_e_login_ok_and_token_works
    kind: function
    at: 'tests/v0_7_auth.rs:L204-L213'
  - symbol: tc_auth_f_logout_invalidates_token
    kind: function
    at: 'tests/v0_7_auth.rs:L217-L225'
  - symbol: tc_auth_g_disabled_user_token_401
    kind: function
    at: 'tests/v0_7_auth.rs:L229-L252'
  - symbol: FakeEngine
    kind: struct
    at: 'tests/v0_8_persistence.rs:L26-L26'
  - symbol: name
    kind: function
    at: 'tests/v0_8_persistence.rs:L30-L32'
  - symbol: reason
    kind: function
    at: 'tests/v0_8_persistence.rs:L33-L55'
  - symbol: spawn_server
    kind: function
    at: 'tests/v0_8_persistence.rs:L59-L79'
  - symbol: http_call
    kind: function
    at: 'tests/v0_8_persistence.rs:L82-L117'
  - symbol: wait_run_done
    kind: function
    at: 'tests/v0_8_persistence.rs:L120-L130'
  - symbol: temp_db
    kind: function
    at: 'tests/v0_8_persistence.rs:L133-L141'
  - symbol: tc_persist_a_run_lifecycle_persisted
    kind: function
    at: 'tests/v0_8_persistence.rs:L145-L179'
  - symbol: tc_persist_b_restart_rebuilds_run_map
    kind: function
    at: 'tests/v0_8_persistence.rs:L183-L239'
  - symbol: tc_persist_c_crash_running_to_interrupted
    kind: function
    at: 'tests/v0_8_persistence.rs:L243-L278'
  - symbol: tc_persist_d_orphan_pending_to_expired
    kind: function
    at: 'tests/v0_8_persistence.rs:L282-L320'
---
<!-- context:generated:start -->
## Summary

Integration tests for v0.7 RBAC and v0.8 persistence. Auth tests (TC-auth-a..g) exercise login/logout/me endpoints, role-based 403s, view clamping, and cascading session invalidation on user disable. Persistence tests (TC-persist-a..d) verify run lifecycle states (Running/Completed/Interrupted) and approval states (Pending/Expired) survive restarts, with crashed runs marked Interrupted and orphaned approvals flipped to Expired on recovery.

## Related

- validates [[auth-rbac]] — Exercises login/logout, role gating, view clamping, session invalidation.
- validates [[persistence-layer]] — Verifies run/approval state survives restarts via file-based DB.
- validates [[server-api]] — Spawns real HTTP servers and exercises the full runtime path.
<!-- context:generated:end -->

## Notes

_Anything written below the generated block is preserved when the graph is regenerated._
