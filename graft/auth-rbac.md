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
