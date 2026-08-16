---
name: Auth & RBAC
slug: auth-rbac
type: system
sources:
  - path: src/auth.rs
    hash: f40165b6efe820bd1726fd6245408fdf3b7218c1d8dec3b2f6a5610ab4d08a79
sources_digest: 4a14e07890b8a8de73b48b34b0dec5fa64b413e83aa421d8c6c093c1ca2b0b19
links:
  - to: persistence-layer
    relation: uses
    description: Backed by LibSQLStorage wb_user and wb_session tables.
  - to: server-api
    relation: uses
    description: Server middleware enforces auth and injects AuthUser into handlers.
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
---
<!-- context:generated:start -->
## Summary

Three-tier role hierarchy (Executive < Operator < Developer) with numeric rank-based permission checks and server-side view clamping. LocalIdentityProvider uses argon2id against wb_user; SessionStore issues opaque random tokens in wb_session with 12h TTL, immediate revocation, and cleanup of expired/orphaned sessions. Returns None for both unknown users and wrong passwords to prevent user enumeration; idempotent admin bootstrap.

## Related

- uses [[persistence-layer]] — Backed by LibSQLStorage wb_user and wb_session tables.
- uses [[server-api]] — Server middleware enforces auth and injects AuthUser into handlers.
<!-- context:generated:end -->

## Notes

_Anything written below the generated block is preserved when the graph is regenerated._
