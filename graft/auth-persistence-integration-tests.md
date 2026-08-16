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
  - symbol: common
    kind: module
    at: 'tests/v0_7_auth.rs:L6-L6'
  - symbol: login
    kind: function
    at: 'tests/v0_7_auth.rs:L19-L28'
  - symbol: spawn_auth_server
    kind: function
    at: 'tests/v0_7_auth.rs:L32-L66'
  - symbol: tc_auth_a_no_token_401
    kind: function
    at: 'tests/v0_7_auth.rs:L71-L82'
  - symbol: tc_auth_b_wrong_token_401
    kind: function
    at: 'tests/v0_7_auth.rs:L86-L97'
  - symbol: tc_auth_c_executive_run_forbidden
    kind: function
    at: 'tests/v0_7_auth.rs:L101-L114'
  - symbol: tc_auth_d_view_clamped_to_role
    kind: function
    at: 'tests/v0_7_auth.rs:L118-L139'
  - symbol: tc_auth_e_login_ok_and_token_works
    kind: function
    at: 'tests/v0_7_auth.rs:L143-L152'
  - symbol: tc_auth_f_logout_invalidates_token
    kind: function
    at: 'tests/v0_7_auth.rs:L156-L165'
  - symbol: tc_auth_g_disabled_user_token_401
    kind: function
    at: 'tests/v0_7_auth.rs:L169-L192'
  - symbol: tc_auth_h_graph_clamped_to_role
    kind: function
    at: 'tests/v0_7_auth.rs:L197-L231'
  - symbol: tc_auth_i_trace_list_clamped_to_role
    kind: function
    at: 'tests/v0_7_auth.rs:L236-L293'
  - symbol: tc_auth_j_login_rate_limited
    kind: function
    at: 'tests/v0_7_auth.rs:L297-L308'
  - symbol: tc_auth_k_session_token_hashed_at_rest
    kind: function
    at: 'tests/v0_7_auth.rs:L312-L322'
  - symbol: common
    kind: module
    at: 'tests/v0_8_persistence.rs:L10-L10'
  - symbol: wait_run_done
    kind: function
    at: 'tests/v0_8_persistence.rs:L24-L34'
  - symbol: temp_db
    kind: function
    at: 'tests/v0_8_persistence.rs:L37-L45'
  - symbol: tc_persist_a_run_lifecycle_persisted
    kind: function
    at: 'tests/v0_8_persistence.rs:L50-L82'
  - symbol: tc_persist_b_restart_rebuilds_run_map
    kind: function
    at: 'tests/v0_8_persistence.rs:L86-L132'
  - symbol: tc_persist_c_crash_running_to_interrupted
    kind: function
    at: 'tests/v0_8_persistence.rs:L136-L157'
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
