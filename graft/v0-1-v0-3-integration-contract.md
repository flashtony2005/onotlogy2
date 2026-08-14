---
name: V0.1–V0.3 Integration Contract
slug: v0-1-v0-3-integration-contract
type: system
sources:
  - path: tests/v0_1.rs
    hash: 2bf0838233b72b77468f7f936fc325cf82d469c91e1a0aeced877881f2f11442
  - path: tests/v0_2.rs
    hash: 39757773749ee020a794fed76465fcad36e3718f2705eb2c076281e853e6149b
  - path: tests/v0_3.rs
    hash: 36a45bc6af635916871b833f07de61d530477a4f6b3836010a92692759ccc5d2
sources_digest: 66d8abb9d802da1b0585e75d6a1c0870af74dcdc4b9946c66c189178f32a7d0e
links:
  - to: ontology-ir-odl-parsing
    relation: validates
    description: Asserts parse_odl error codes and IR fidelity.
  - to: persistence-layer
    relation: validates
    description: 'Tests CRUD, versioning, WB2101 stale-schema rejection.'
  - to: planner-policy-engine
    relation: validates
    description: 'Drives Planner with FakeEngine, tests policy enforcement and approvals.'
  - to: runtime-kernel-observability
    relation: validates
    description: 'Verifies trace events, object_refs, event bus consistency.'
generator:
  version: 1
covers:
  - symbol: tc001_parse_valid
    kind: function
    at: 'tests/v0_1.rs:L13-L26'
  - symbol: tc001b_parse_invalid
    kind: function
    at: 'tests/v0_1.rs:L29-L41'
  - symbol: tc002_ir_fidelity
    kind: function
    at: 'tests/v0_1.rs:L45-L52'
  - symbol: tc003_codegen
    kind: function
    at: 'tests/v0_1.rs:L56-L69'
  - symbol: tc004_object_crud
    kind: function
    at: 'tests/v0_1.rs:L73-L92'
  - symbol: tc005_turso_roundtrip
    kind: function
    at: 'tests/v0_1.rs:L96-L109'
  - symbol: tc006_render_list
    kind: function
    at: 'tests/v0_1.rs:L113-L130'
  - symbol: tc007_world_trace
    kind: function
    at: 'tests/v0_1.rs:L134-L161'
  - symbol: tc007b_missing_refs_blocked
    kind: function
    at: 'tests/v0_1.rs:L164-L184'
  - symbol: tc008_bus_publish_consistent
    kind: function
    at: 'tests/v0_1.rs:L188-L203'
  - symbol: tc005b_open_rejects_legacy_schema
    kind: function
    at: 'tests/v0_1.rs:L207-L235'
  - symbol: tc012_relation_link_and_neighbors
    kind: function
    at: 'tests/v0_2.rs:L9-L42'
  - symbol: tc012b_duplicate_edge_rejected
    kind: function
    at: 'tests/v0_2.rs:L46-L70'
  - symbol: tc013_domain_event_emitted
    kind: function
    at: 'tests/v0_2.rs:L74-L101'
  - symbol: FakeEngine
    kind: struct
    at: 'tests/v0_3.rs:L31-L31'
  - symbol: name
    kind: function
    at: 'tests/v0_3.rs:L35-L37'
  - symbol: reason
    kind: function
    at: 'tests/v0_3.rs:L38-L60'
  - symbol: UnavailableEngine
    kind: struct
    at: 'tests/v0_3.rs:L64-L64'
  - symbol: name
    kind: function
    at: 'tests/v0_3.rs:L68-L70'
  - symbol: reason
    kind: function
    at: 'tests/v0_3.rs:L71-L75'
  - symbol: health
    kind: function
    at: 'tests/v0_3.rs:L76-L78'
  - symbol: seed_and_run
    kind: function
    at: 'tests/v0_3.rs:L82-L95'
  - symbol: tc010_cognitive_trace_with_refs
    kind: function
    at: 'tests/v0_3.rs:L99-L134'
  - symbol: tc011_planner_closed_loop
    kind: function
    at: 'tests/v0_3.rs:L138-L168'
  - symbol: tc010b_engine_unavailable_wb2004
    kind: function
    at: 'tests/v0_3.rs:L172-L189'
  - symbol: tc011b_policy_deny_wb2005
    kind: function
    at: 'tests/v0_3.rs:L193-L204'
  - symbol: tc009_three_view_filtering
    kind: function
    at: 'tests/v0_3.rs:L208-L259'
  - symbol: tc009_ws_stream_and_reconnect
    kind: function
    at: 'tests/v0_3.rs:L263-L327'
  - symbol: collect_ws
    kind: function
    at: 'tests/v0_3.rs:L330-L352'
  - symbol: http_call
    kind: function
    at: 'tests/v0_3.rs:L355-L390'
  - symbol: spawn_server
    kind: function
    at: 'tests/v0_3.rs:L393-L413'
  - symbol: tc012_multi_candidate_closed_loop
    kind: function
    at: 'tests/v0_3.rs:L417-L440'
  - symbol: tc011_policy_allow_non_destructive
    kind: function
    at: 'tests/v0_3.rs:L444-L487'
  - symbol: tc012b_empty_world_object_refs_sentinel
    kind: function
    at: 'tests/v0_3.rs:L491-L520'
  - symbol: tc013_trace_query_filters
    kind: function
    at: 'tests/v0_3.rs:L524-L560'
  - symbol: tc_rest_trace_list_and_404
    kind: function
    at: 'tests/v0_3.rs:L564-L596'
  - symbol: tc_rest_run_start_gate
    kind: function
    at: 'tests/v0_3.rs:L600-L653'
  - symbol: tc_ws_run_scoped
    kind: function
    at: 'tests/v0_3.rs:L657-L694'
  - symbol: tc011_repeated_run_is_idempotent
    kind: function
    at: 'tests/v0_3.rs:L698-L712'
  - symbol: ODL_ACTIONS
    kind: constant
    at: 'tests/v0_3.rs:L715-L736'
  - symbol: ODL_RETAIN
    kind: constant
    at: 'tests/v0_3.rs:L738-L751'
  - symbol: ODL_REFUND
    kind: constant
    at: 'tests/v0_3.rs:L754-L775'
  - symbol: ODL_APPROVE
    kind: constant
    at: 'tests/v0_3.rs:L778-L787'
  - symbol: tc014a_odl_parses_actions
    kind: function
    at: 'tests/v0_3.rs:L791-L826'
  - symbol: tc014b_action_engine_executes
    kind: function
    at: 'tests/v0_3.rs:L830-L868'
  - symbol: tc014c_planner_via_registry_closed_loop
    kind: function
    at: 'tests/v0_3.rs:L872-L922'
  - symbol: tc022a_memory_store_roundtrip
    kind: function
    at: 'tests/v0_3.rs:L926-L949'
  - symbol: tc022b_planner_memory_closed_loop
    kind: function
    at: 'tests/v0_3.rs:L953-L1003'
  - symbol: tc015a_odl_parses_policy
    kind: function
    at: 'tests/v0_3.rs:L1007-L1030'
  - symbol: tc015b_approval_gate
    kind: function
    at: 'tests/v0_3.rs:L1034-L1087'
  - symbol: tc015c_precondition_eval
    kind: function
    at: 'tests/v0_3.rs:L1091-L1178'
  - symbol: tc015d_planner_policy_gate
    kind: function
    at: 'tests/v0_3.rs:L1182-L1225'
  - symbol: tc021a_cli_auto_approve_flag_exists
    kind: function
    at: 'tests/v0_3.rs:L1229-L1252'
  - symbol: tc021b_console_approver_constructible
    kind: function
    at: 'tests/v0_3.rs:L1256-L1259'
  - symbol: tc023a_server_http_approval_suspend_and_approve
    kind: function
    at: 'tests/v0_3.rs:L1267-L1372'
  - symbol: tc023b_server_http_approval_reject
    kind: function
    at: 'tests/v0_3.rs:L1376-L1500'
  - symbol: tc023c_server_http_approval_timeout_expired
    kind: function
    at: 'tests/v0_3.rs:L1506-L1631'
  - symbol: tc023d_timeout_expired_distinct_from_rejected
    kind: function
    at: 'tests/v0_3.rs:L1635-L1660'
  - symbol: tc023e_approval_wrong_run_id_rejected
    kind: function
    at: 'tests/v0_3.rs:L1664-L1748'
  - symbol: tc024a_server_memory_endpoint
    kind: function
    at: 'tests/v0_3.rs:L1752-L1787'
  - symbol: tc025a_experience_mining
    kind: function
    at: 'tests/v0_3.rs:L1797-L1822'
  - symbol: tc025b_confidence_boundary
    kind: function
    at: 'tests/v0_3.rs:L1826-L1833'
  - symbol: tc025c_proposal_does_not_change_behavior
    kind: function
    at: 'tests/v0_3.rs:L1837-L1854'
  - symbol: tc025d_validation_threshold
    kind: function
    at: 'tests/v0_3.rs:L1858-L1888'
  - symbol: tc025e_evolution_does_not_bypass_approval
    kind: function
    at: 'tests/v0_3.rs:L1892-L1947'
  - symbol: tc025f_evolution_idempotent
    kind: function
    at: 'tests/v0_3.rs:L1951-L1979'
  - symbol: tc025g_evolution_traceability
    kind: function
    at: 'tests/v0_3.rs:L1983-L2018'
  - symbol: tc025h_server_evolution_endpoint
    kind: function
    at: 'tests/v0_3.rs:L2022-L2049'
  - symbol: tc025i_revoke_learned
    kind: function
    at: 'tests/v0_3.rs:L2053-L2071'
  - symbol: mk_learned
    kind: function
    at: 'tests/v0_3.rs:L2078-L2089'
  - symbol: refund_step
    kind: function
    at: 'tests/v0_3.rs:L2092-L2103'
  - symbol: refund_def
    kind: function
    at: 'tests/v0_3.rs:L2106-L2116'
  - symbol: tc026a_default_off_still_deny
    kind: function
    at: 'tests/v0_3.rs:L2120-L2153'
  - symbol: tc026b_switch_off_falls_back
    kind: function
    at: 'tests/v0_3.rs:L2157-L2174'
  - symbol: tc026c_confidence_boundary
    kind: function
    at: 'tests/v0_3.rs:L2178-L2213'
  - symbol: tc026d_runs_boundary
    kind: function
    at: 'tests/v0_3.rs:L2217-L2252'
  - symbol: tc026e_all_gates_pass_auto_approve
    kind: function
    at: 'tests/v0_3.rs:L2256-L2297'
  - symbol: tc026f_failure_blocks_auto_approve
    kind: function
    at: 'tests/v0_3.rs:L2301-L2348'
  - symbol: FailingAuditor
    kind: struct
    at: 'tests/v0_3.rs:L2351-L2351'
  - symbol: record
    kind: function
    at: 'tests/v0_3.rs:L2354-L2356'
  - symbol: tc026g_audit_failure_falls_back
    kind: function
    at: 'tests/v0_3.rs:L2361-L2406'
  - symbol: tc026h_audit_and_revoke_reversible
    kind: function
    at: 'tests/v0_3.rs:L2410-L2512'
---
<!-- context:generated:start -->
## Summary

The integration test suites locking the core pipeline contracts: ODL parsing error codes (1001/1002/1005), IR roundtrip fidelity, storage CRUD with version increments, runtime tracing with object_refs, event bus consistency, relation linking with WB2007 duplicate rejection, domain events, planner-driven runs with FakeEngine/UnavailableEngine, three-tier view filtering, WS reconnection, and REST error codes (WB1001/WB2002/WB2004/WB2005). TC-005-b verifies legacy schema files return WB2101 rather than cryptic failures.

## Related

- validates [[ontology-ir-odl-parsing]] — Asserts parse_odl error codes and IR fidelity.
- validates [[persistence-layer]] — Tests CRUD, versioning, WB2101 stale-schema rejection.
- validates [[planner-policy-engine]] — Drives Planner with FakeEngine, tests policy enforcement and approvals.
- validates [[runtime-kernel-observability]] — Verifies trace events, object_refs, event bus consistency.
<!-- context:generated:end -->

## Notes

_Anything written below the generated block is preserved when the graph is regenerated._
