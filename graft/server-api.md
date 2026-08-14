---
name: Server & API
slug: server-api
type: system
sources:
  - path: src/server.rs
    hash: 453cc3dbe5459733726e6d4412099744eda5e67d77abf9f5b946c84a670b5c4a
sources_digest: 42cb53b408eb7592b30a2791f305902bd86ce0c046ad4b88e85f1fd1ab74f6e0
links:
  - to: auth-rbac
    relation: uses
    description: Auth middleware enforces roles and injects AuthUser.
  - to: planner-policy-engine
    relation: uses
    description: ServerApprover handles HTTP-based approval; Planner drives runs.
  - to: runtime-kernel-observability
    relation: uses
    description: Integrates RuntimeKernel for run state and event streaming.
generator:
  version: 1
covers:
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
---
<!-- context:generated:start -->
## Summary

The axum HTTP/WebSocket server exposing REST endpoints for traces, timelines, memory, evolution, and approvals, plus WS streaming with `?after` reconnection. Enforces auth via middleware, uses a unified ApiResponse envelope with WB error codes, and rebuilds in-memory run state from persisted storage on restart. Serves static UI from ui/dist. Notable gotchas: dual approval paths (legacy HTTP vs evolution auto-approve) and camelCase DTOs as single source of truth for both WS and timeline.

## Related

- uses [[auth-rbac]] — Auth middleware enforces roles and injects AuthUser.
- uses [[planner-policy-engine]] — ServerApprover handles HTTP-based approval; Planner drives runs.
- uses [[runtime-kernel-observability]] — Integrates RuntimeKernel for run state and event streaming.
<!-- context:generated:end -->

## Notes

_Anything written below the generated block is preserved when the graph is regenerated._
