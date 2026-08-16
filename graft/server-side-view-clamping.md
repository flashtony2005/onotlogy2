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
  - symbol: can_read_evaluation
    kind: function
    at: 'src/auth.rs:L112-L114'
  - symbol: AuthError
    kind: enum
    at: 'src/auth.rs:L119-L126'
  - symbol: IdentityProvider
    kind: interface
    at: 'src/auth.rs:L130-L139'
  - symbol: authenticate
    kind: function
    at: 'src/auth.rs:L132-L136'
  - symbol: bootstrap_admin
    kind: function
    at: 'src/auth.rs:L138-L138'
  - symbol: argon2
    kind: function
    at: 'src/auth.rs:L144-L150'
  - symbol: LocalIdentityProvider
    kind: struct
    at: 'src/auth.rs:L153-L155'
  - symbol: new
    kind: function
    at: 'src/auth.rs:L158-L160'
  - symbol: hash_password
    kind: function
    at: 'src/auth.rs:L163-L169'
  - symbol: authenticate
    kind: function
    at: 'src/auth.rs:L174-L199'
  - symbol: bootstrap_admin
    kind: function
    at: 'src/auth.rs:L201-L225'
  - symbol: SessionStore
    kind: struct
    at: 'src/auth.rs:L232-L234'
  - symbol: hash_token
    kind: function
    at: 'src/auth.rs:L237-L242'
  - symbol: new
    kind: function
    at: 'src/auth.rs:L245-L247'
  - symbol: TTL_SECS
    kind: constant
    at: 'src/auth.rs:L250-L250'
  - symbol: issue
    kind: function
    at: 'src/auth.rs:L253-L263'
  - symbol: validate
    kind: function
    at: 'src/auth.rs:L266-L298'
  - symbol: revoke
    kind: function
    at: 'src/auth.rs:L301-L306'
  - symbol: now_secs
    kind: function
    at: 'src/auth.rs:L310-L315'
  - symbol: tests
    kind: module
    at: 'src/auth.rs:L318-L393'
  - symbol: mk_storage
    kind: function
    at: 'src/auth.rs:L322-L324'
  - symbol: role_rank_orders_executive_operator_developer
    kind: function
    at: 'src/auth.rs:L327-L333'
  - symbol: view_clamp_uses_role_rank_not_string
    kind: function
    at: 'src/auth.rs:L336-L352'
  - symbol: local_identity_bootstrap_authenticate_and_disable
    kind: function
    at: 'src/auth.rs:L355-L371'
  - symbol: session_issue_validate_revoke_roundtrip
    kind: function
    at: 'src/auth.rs:L374-L392'
  - symbol: toTimelineEvent
    kind: function
    at: 'ui/src/view.ts:L25-L36'
  - symbol: asString
    kind: function
    at: 'ui/src/view.ts:L38-L41'
  - symbol: deriveState
    kind: function
    at: 'ui/src/view.ts:L45-L55'
  - symbol: buildTimeline
    kind: function
    at: 'ui/src/view.ts:L57-L110'
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
