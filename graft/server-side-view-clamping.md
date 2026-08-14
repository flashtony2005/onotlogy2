---
name: Server-Side View Clamping
slug: server-side-view-clamping
type: concept
sources:
  - path: src/auth.rs
    hash: f40165b6efe820bd1726fd6245408fdf3b7218c1d8dec3b2f6a5610ab4d08a79
  - path: src/server.rs
    hash: 453cc3dbe5459733726e6d4412099744eda5e67d77abf9f5b946c84a670b5c4a
  - path: src/trace.rs
    hash: 9bf0f678764ca231eca4ffed0cb6a4405cbeef02a0883444ceee3397c224d79e
  - path: ui/src/view.ts
    hash: 0f697da5354c4991ca2e686c4611581aa9220e39162a9cad77b4ba2f9624f4e6
sources_digest: 11e57618d2ca7f3be7914c5477fb1b86565977de0cd86c3e7d07c60cfa1c7280
links:
  - to: auth-rbac
    relation: implements
    description: Role.max_view defines the clamp boundary.
  - to: timeline-views-dtos
    relation: implements
    description: build_timeline filters per enforced view.
generator:
  version: 1
covers:
  - symbol: Role
    kind: enum
    at: 'src/auth.rs:L26-L30'
  - symbol: rank
    kind: function
    at: 'src/auth.rs:L35-L41'
  - symbol: max_view
    kind: function
    at: 'src/auth.rs:L44-L50'
  - symbol: Err
    kind: type
    at: 'src/auth.rs:L54-L54'
  - symbol: from_str
    kind: function
    at: 'src/auth.rs:L55-L62'
  - symbol: clamp_view
    kind: function
    at: 'src/auth.rs:L67-L75'
  - symbol: view_rank
    kind: function
    at: 'src/auth.rs:L78-L84'
  - symbol: AuthUser
    kind: struct
    at: 'src/auth.rs:L88-L92'
  - symbol: can_trigger_run
    kind: function
    at: 'src/auth.rs:L96-L98'
  - symbol: can_resolve_approval
    kind: function
    at: 'src/auth.rs:L100-L102'
  - symbol: can_write_evolution
    kind: function
    at: 'src/auth.rs:L104-L106'
  - symbol: can_read_audit
    kind: function
    at: 'src/auth.rs:L108-L110'
  - symbol: AuthError
    kind: enum
    at: 'src/auth.rs:L115-L122'
  - symbol: IdentityProvider
    kind: interface
    at: 'src/auth.rs:L126-L135'
  - symbol: authenticate
    kind: function
    at: 'src/auth.rs:L128-L132'
  - symbol: bootstrap_admin
    kind: function
    at: 'src/auth.rs:L134-L134'
  - symbol: LocalIdentityProvider
    kind: struct
    at: 'src/auth.rs:L138-L140'
  - symbol: new
    kind: function
    at: 'src/auth.rs:L143-L145'
  - symbol: hash_password
    kind: function
    at: 'src/auth.rs:L148-L154'
  - symbol: authenticate
    kind: function
    at: 'src/auth.rs:L159-L187'
  - symbol: bootstrap_admin
    kind: function
    at: 'src/auth.rs:L189-L213'
  - symbol: SessionStore
    kind: struct
    at: 'src/auth.rs:L217-L219'
  - symbol: new
    kind: function
    at: 'src/auth.rs:L222-L224'
  - symbol: TTL_SECS
    kind: constant
    at: 'src/auth.rs:L227-L227'
  - symbol: issue
    kind: function
    at: 'src/auth.rs:L230-L240'
  - symbol: validate
    kind: function
    at: 'src/auth.rs:L243-L274'
  - symbol: revoke
    kind: function
    at: 'src/auth.rs:L277-L282'
  - symbol: now_secs
    kind: function
    at: 'src/auth.rs:L286-L291'
  - symbol: tests
    kind: module
    at: 'src/auth.rs:L294-L369'
  - symbol: mk_storage
    kind: function
    at: 'src/auth.rs:L298-L300'
  - symbol: role_rank_orders_executive_operator_developer
    kind: function
    at: 'src/auth.rs:L303-L309'
  - symbol: view_clamp_uses_role_rank_not_string
    kind: function
    at: 'src/auth.rs:L312-L328'
  - symbol: local_identity_bootstrap_authenticate_and_disable
    kind: function
    at: 'src/auth.rs:L331-L347'
  - symbol: session_issue_validate_revoke_roundtrip
    kind: function
    at: 'src/auth.rs:L350-L368'
  - symbol: RunRecord
    kind: struct
    at: 'src/server.rs:L43-L47'
  - symbol: AppState
    kind: struct
    at: 'src/server.rs:L50-L71'
  - symbol: ApiResponse
    kind: struct
    at: 'src/server.rs:L75-L80'
  - symbol: ok
    kind: function
    at: 'src/server.rs:L83-L89'
  - symbol: error
    kind: function
    at: 'src/server.rs:L94-L100'
  - symbol: wb_code
    kind: function
    at: 'src/server.rs:L104-L106'
  - symbol: now_micros
    kind: function
    at: 'src/server.rs:L109-L115'
  - symbol: TraceListParams
    kind: struct
    at: 'src/server.rs:L119-L134'
  - symbol: WsParams
    kind: struct
    at: 'src/server.rs:L137-L140'
  - symbol: ViewParam
    kind: struct
    at: 'src/server.rs:L143-L146'
  - symbol: RunBody
    kind: struct
    at: 'src/server.rs:L150-L158'
  - symbol: TraceListDto
    kind: struct
    at: 'src/server.rs:L162-L165'
  - symbol: RunStartDto
    kind: struct
    at: 'src/server.rs:L169-L173'
  - symbol: MemoryRecordDto
    kind: struct
    at: 'src/server.rs:L178-L184'
  - symbol: MemoryListDto
    kind: struct
    at: 'src/server.rs:L188-L191'
  - symbol: PatternDto
    kind: struct
    at: 'src/server.rs:L197-L203'
  - symbol: ProposalDto
    kind: struct
    at: 'src/server.rs:L207-L215'
  - symbol: LearnedDto
    kind: struct
    at: 'src/server.rs:L219-L229'
  - symbol: EvolutionDto
    kind: struct
    at: 'src/server.rs:L233-L237'
  - symbol: router
    kind: function
    at: 'src/server.rs:L242-L276'
  - symbol: serve
    kind: function
    at: 'src/server.rs:L283-L305'
  - symbol: serve_with_timeout
    kind: function
    at: 'src/server.rs:L311-L347'
  - symbol: recover
    kind: function
    at: 'src/server.rs:L351-L366'
  - symbol: auth_layer
    kind: function
    at: 'src/server.rs:L373-L403'
  - symbol: is_public
    kind: function
    at: 'src/server.rs:L406-L408'
  - symbol: bearer_token
    kind: function
    at: 'src/server.rs:L411-L415'
  - symbol: unauthorized
    kind: function
    at: 'src/server.rs:L418-L420'
  - symbol: forbidden
    kind: function
    at: 'src/server.rs:L423-L425'
  - symbol: AuthToken
    kind: struct
    at: 'src/server.rs:L429-L429'
  - symbol: LoginBody
    kind: struct
    at: 'src/server.rs:L433-L436'
  - symbol: auth_login
    kind: function
    at: 'src/server.rs:L439-L456'
  - symbol: auth_logout
    kind: function
    at: 'src/server.rs:L459-L467'
  - symbol: auth_me
    kind: function
    at: 'src/server.rs:L470-L476'
  - symbol: list_trace
    kind: function
    at: 'src/server.rs:L480-L504'
  - symbol: get_trace
    kind: function
    at: 'src/server.rs:L507-L517'
  - symbol: timeline
    kind: function
    at: 'src/server.rs:L520-L552'
  - symbol: list_memory
    kind: function
    at: 'src/server.rs:L555-L575'
  - symbol: list_evolution
    kind: function
    at: 'src/server.rs:L579-L642'
  - symbol: revoke_learned
    kind: function
    at: 'src/server.rs:L645-L658'
  - symbol: AutoApproveBody
    kind: struct
    at: 'src/server.rs:L664-L667'
  - symbol: toggle_auto_approve
    kind: function
    at: 'src/server.rs:L669-L701'
  - symbol: ApprovalAuditDto
    kind: struct
    at: 'src/server.rs:L706-L714'
  - symbol: from
    kind: function
    at: 'src/server.rs:L717-L727'
  - symbol: list_approval_audit_handler
    kind: function
    at: 'src/server.rs:L731-L748'
  - symbol: list_approvals
    kind: function
    at: 'src/server.rs:L751-L773'
  - symbol: ApprovalDecisionBody
    kind: struct
    at: 'src/server.rs:L776-L779'
  - symbol: resolve_approval
    kind: function
    at: 'src/server.rs:L782-L818'
  - symbol: approval_record_to_request
    kind: function
    at: 'src/server.rs:L821-L830'
  - symbol: run_start
    kind: function
    at: 'src/server.rs:L833-L928'
  - symbol: ws_handler
    kind: function
    at: 'src/server.rs:L931-L939'
  - symbol: handle_ws
    kind: function
    at: 'src/server.rs:L942-L989'
  - symbol: collect_history
    kind: function
    at: 'src/server.rs:L992-L1019'
  - symbol: serve_static
    kind: function
    at: 'src/server.rs:L1022-L1060'
  - symbol: content_type
    kind: function
    at: 'src/server.rs:L1063-L1085'
  - symbol: tests
    kind: module
    at: 'src/server.rs:L1088-L1104'
  - symbol: run_start_dto_is_camel_case
    kind: function
    at: 'src/server.rs:L1092-L1103'
  - symbol: COGNITIVE_STEPS
    kind: constant
    at: 'src/trace.rs:L12-L21'
  - symbol: View
    kind: enum
    at: 'src/trace.rs:L25-L32'
  - symbol: name
    kind: function
    at: 'src/trace.rs:L36-L42'
  - symbol: from_param
    kind: function
    at: 'src/trace.rs:L45-L51'
  - symbol: TraceEventDto
    kind: struct
    at: 'src/trace.rs:L57-L76'
  - symbol: from
    kind: function
    at: 'src/trace.rs:L79-L93'
  - symbol: TimelineEvent
    kind: struct
    at: 'src/trace.rs:L99-L114'
  - symbol: DecisionSummary
    kind: struct
    at: 'src/trace.rs:L118-L122'
  - symbol: RunTimeline
    kind: struct
    at: 'src/trace.rs:L127-L139'
  - symbol: derive_run_state
    kind: function
    at: 'src/trace.rs:L142-L159'
  - symbol: to_timeline_event
    kind: function
    at: 'src/trace.rs:L162-L173'
  - symbol: build_timeline
    kind: function
    at: 'src/trace.rs:L176-L248'
  - symbol: DecisionText
    kind: interface
    at: 'src/trace.rs:L251-L253'
  - symbol: description_or_output
    kind: function
    at: 'src/trace.rs:L252-L252'
  - symbol: description_or_output
    kind: function
    at: 'src/trace.rs:L255-L262'
  - symbol: tests
    kind: module
    at: 'src/trace.rs:L266-L300'
  - symbol: timeline_dto_is_camel_case
    kind: function
    at: 'src/trace.rs:L270-L299'
  - symbol: toTimelineEvent
    kind: function
    at: 'ui/src/view.ts:L24-L35'
  - symbol: asString
    kind: function
    at: 'ui/src/view.ts:L37-L40'
  - symbol: buildTimeline
    kind: function
    at: 'ui/src/view.ts:L42-L97'
---
<!-- context:generated:start -->
## Summary

A security invariant: the server never trusts the client's requested view — the returned RunTimeline.view reflects the enforced role, not the request. AuthUser.max_view clamps the requested view, and the UI mirrors this logic client-side (ui/src/view.ts) so WS real-time streams stay consistent with REST queries. Executive cannot see reasoning/input/output details even if requested.

## Related

- implements [[auth-rbac]] — Role.max_view defines the clamp boundary.
- implements [[timeline-views-dtos]] — build_timeline filters per enforced view.
<!-- context:generated:end -->

## Notes

_Anything written below the generated block is preserved when the graph is regenerated._
