---
name: Idempotency & Stable IDs
slug: idempotency-stable-ids
type: concept
sources:
  - path: src/action.rs
    hash: 64d2dce7c3f6d1748ef7c756476661d8cf31ff8a60324ddeb38069754cfeac6a
  - path: src/evolution.rs
    hash: 8b22e1db63cf8cb31cf6cdc3f9664b067373f58a2c20046e9cfc21657c44cef0
  - path: src/storage.rs
    hash: e814f6bb191298d5ddc7cbfabd21208a16f55ce5c5464b6aeb8662f2c1b43439
  - path: tests/v0_3.rs
    hash: 36a45bc6af635916871b833f07de61d530477a4f6b3836010a92692759ccc5d2
sources_digest: 19279c1b2ec548149c85de159a2b62c62905a65646beb6ed975fec674c6be334
links:
  - to: action-engine
    relation: implements
    description: Upserts and deduplicated edges.
  - to: evolution-engine
    relation: implements
    description: Stable proposal/learned IDs.
generator:
  version: 1
covers:
  - symbol: ActionRegistry
    kind: struct
    at: 'src/action.rs:L20-L22'
  - symbol: empty
    kind: function
    at: 'src/action.rs:L26-L28'
  - symbol: from_ir
    kind: function
    at: 'src/action.rs:L31-L41'
  - symbol: get
    kind: function
    at: 'src/action.rs:L44-L46'
  - symbol: actions_of
    kind: function
    at: 'src/action.rs:L49-L54'
  - symbol: has_actions
    kind: function
    at: 'src/action.rs:L57-L59'
  - symbol: is_empty
    kind: function
    at: 'src/action.rs:L62-L64'
  - symbol: ActionError
    kind: enum
    at: 'src/action.rs:L69-L78'
  - symbol: ActionEffect
    kind: struct
    at: 'src/action.rs:L82-L89'
  - symbol: ActionEngine
    kind: struct
    at: 'src/action.rs:L92-L94'
  - symbol: new
    kind: function
    at: 'src/action.rs:L97-L99'
  - symbol: invoke
    kind: function
    at: 'src/action.rs:L103-L182'
  - symbol: resolve
    kind: function
    at: 'src/action.rs:L186-L193'
  - symbol: PATTERN_MIN_HITS
    kind: constant
    at: 'src/evolution.rs:L27-L27'
  - symbol: CONFIDENCE_DIVISOR
    kind: constant
    at: 'src/evolution.rs:L29-L29'
  - symbol: VALIDATE_THRESHOLD
    kind: constant
    at: 'src/evolution.rs:L31-L31'
  - symbol: pattern_label_of
    kind: function
    at: 'src/evolution.rs:L35-L37'
  - symbol: proposal_id_of
    kind: function
    at: 'src/evolution.rs:L40-L42'
  - symbol: learned_id_of
    kind: function
    at: 'src/evolution.rs:L45-L47'
  - symbol: Experience
    kind: struct
    at: 'src/evolution.rs:L52-L55'
  - symbol: action
    kind: function
    at: 'src/evolution.rs:L59-L64'
  - symbol: Pattern
    kind: struct
    at: 'src/evolution.rs:L69-L80'
  - symbol: confidence_of
    kind: function
    at: 'src/evolution.rs:L87-L89'
  - symbol: ProposalKind
    kind: enum
    at: 'src/evolution.rs:L94-L96'
  - symbol: ProposalStatus
    kind: enum
    at: 'src/evolution.rs:L100-L104'
  - symbol: Proposal
    kind: struct
    at: 'src/evolution.rs:L109-L121'
  - symbol: LearnedKnowledge
    kind: struct
    at: 'src/evolution.rs:L126-L141'
  - symbol: default_true
    kind: function
    at: 'src/evolution.rs:L143-L145'
  - symbol: EvolutionState
    kind: struct
    at: 'src/evolution.rs:L149-L153'
  - symbol: mine_patterns
    kind: function
    at: 'src/evolution.rs:L159-L179'
  - symbol: propose
    kind: function
    at: 'src/evolution.rs:L182-L192'
  - symbol: validate
    kind: function
    at: 'src/evolution.rs:L199-L209'
  - symbol: parse_run_ids
    kind: function
    at: 'src/evolution.rs:L212-L214'
  - symbol: encode_run_ids
    kind: function
    at: 'src/evolution.rs:L217-L224'
  - symbol: merge_run_ids
    kind: function
    at: 'src/evolution.rs:L227-L234'
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

A cross-cutting design principle: operations must be safe to repeat. ActionEngine upserts updates and deduplicates link edges; evolution generates stable IDs via proposal_id_of/learned_id_of; memory records upsert with hit counting; repeated runs are idempotent. This enables crash recovery and reconnection without duplicate side effects.

## Related

- implements [[action-engine]] — Upserts and deduplicated edges.
- implements [[evolution-engine]] — Stable proposal/learned IDs.
<!-- context:generated:end -->

## Notes

_Anything written below the generated block is preserved when the graph is regenerated._
