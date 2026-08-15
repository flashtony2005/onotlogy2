# src/auth.rs · [[auth-rbac]] [[server-side-view-clamping]]

Authentication and authorization module implementing local identity, role-based access control, and opaque session tokens.

- Role · enum · L26-L30 — Three-tier permission role enum (Executive < Operator < Developer) with derived ordering.
- rank · function · L35-L41 — Maps each role to a numeric permission level so comparisons use rank instead of strings.
- max_view · function · L44-L50 — Returns the highest view a role is allowed to see, used as the server-side clamp ceiling.
- Err · type · L54-L54 — Associated error type for Role's FromStr, a plain String message.
- from_str · function · L55-L62 — Parses a case-insensitive role name string into a Role, rejecting unknown values.
- clamp_view · function · L67-L75 — Clamps a client-requested view down to the role's allowed maximum, preventing privilege escalation via ?view=.
- view_rank · function · L78-L84 — Maps a View to its numeric permission rank, mirroring Role::rank ordering.
- AuthUser · struct · L88-L92 — Authenticated caller identity injected into request extensions for role-based capability checks.
- can_trigger_run · function · L96-L98 — Grants run-triggering capability to Operator and above (Executive is read-only).
- can_resolve_approval · function · L100-L102 — Grants approval-resolution capability to Operator and above.
- can_write_evolution · function · L104-L106 — Restricts Evolution writes (revoke/auto-approve) to Developer only, enforcing least privilege.
- can_read_audit · function · L108-L110 — Grants audit-read capability to Operator and above.
- AuthError · enum · L115-L122 — Typed error variants for storage, password-hash, and role-parsing failures during auth.
- IdentityProvider · interface · L126-L135 — Pluggable identity source trait (local impl now, OIDC later) exposing authenticate and admin bootstrap.
- authenticate · function · L128-L132 — Trait method to validate credentials and return an AuthUser, or None for unknown/disabled/wrong-password.
- bootstrap_admin · function · L134-L134 — Trait method to create the initial admin account idempotently on first startup.
- LocalIdentityProvider · struct · L138-L140 — Local account identity provider backed by LibSQL storage using argon2id password hashing.
- new · function · L143-L145 — Constructs a LocalIdentityProvider wrapping the given storage.
- hash_password · function · L148-L154 — Computes an argon2id password hash with a random salt, shared by tests/CLI account creation.
- authenticate · function · L159-L187 — Validates a username/password against stored argon2id hash, rejecting disabled users and returning AuthUser on success.
- bootstrap_admin · function · L189-L213 — Idempotently creates the initial Developer-role admin account if the username doesn't already exist.
- SessionStore · struct · L217-L219 — Opaque random-token session storage backed by wb_session, supporting instant revocation without key management.
- new · function · L222-L224 — Constructs a SessionStore wrapping the given storage.
- TTL_SECS · constant · L227-L227 — Default session lifetime of 12 hours.
- issue · function · L230-L240 — Issues a new session by generating a 32-byte random hex token with a 12h expiry and persisting it.
- validate · function · L243-L274 — Validates a token by checking existence, expiry, and user-not-disabled, cleaning up orphaned/expired sessions.
- revoke · function · L277-L282 — Deletes a session token to log the user out immediately.
- now_secs · function · L286-L291 — Returns current unix seconds as i64 for session expiry comparisons.
- tests · module · L294-L369 — Unit tests covering role ordering, view clamping, identity bootstrap/authenticate, and session roundtrip.
- mk_storage · function · L298-L300 — Test helper opening an in-memory LibSQL storage.
- role_rank_orders_executive_operator_developer · function · L303-L309 — Asserts rank ordering matches derived PartialOrd ordering across the three roles.
- view_clamp_uses_role_rank_not_string · function · L312-L328 — Verifies clamp_view clamps higher requests to role ceiling and passes lower/equal requests through.
- local_identity_bootstrap_authenticate_and_disable · function · L331-L347 — Verifies bootstrap idempotency, correct-credential auth, and that wrong/unknown credentials return None.
- session_issue_validate_revoke_roundtrip · function · L350-L368 — Verifies a session token validates after issue and becomes invalid after revoke.
