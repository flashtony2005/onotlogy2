# tests/v0_7_auth.rs · [[auth-persistence-integration-tests]]

Integration test contract for V0.7 auth & RBAC: verifies server-enforced authentication (401 on missing/wrong token), role capability gating (403 for insufficient role), view clamping to role ceiling, and login/logout/disable lifecycle.

- NoopEngine · struct · L21-L21 — Empty test engine that always errors, so auth tests exercise 401/403 paths before any real planner logic runs.
- name · function · L25-L27 — Returns the engine name 'noop' for the test stub.
- reason · function · L28-L30 — Always returns Unavailable error so the engine never produces a real decision during auth tests.
- http_call · function · L34-L75 — Raw HTTP client helper that sends a request with optional Bearer token and parses status plus body from the raw response.
- login · function · L78-L87 — Performs a login POST and extracts the token from the JSON response when status is 200.
- spawn_auth_server · function · L91-L128 — Boots an in-memory auth-enforced server with admin(Developer) and exec(Executive) users, returning the address and shared storage for direct account manipulation.
- tc_auth_a_no_token_401 · function · L132-L143 — Verifies that accessing a protected agent/run endpoint without a token returns 401.
- tc_auth_b_wrong_token_401 · function · L147-L158 — Verifies that a malformed/invalid Bearer token is rejected with 401.
- tc_auth_c_executive_run_forbidden · function · L162-L175 — Verifies that an Executive-role user is denied (403) from triggering agent runs which require Operator or above.
- tc_auth_d_view_clamped_to_role · function · L179-L200 — Verifies the server clamps a frontend-requested ?view=developer down to the Executive role ceiling and strips reasoning/input/output detail from events.
- tc_auth_e_login_ok_and_token_works · function · L204-L213 — Verifies a valid admin login returns 200 with a token and that the token grants access to /auth/me reporting role developer.
- tc_auth_f_logout_invalidates_token · function · L217-L225 — Verifies that after logout, reusing the same token on a protected endpoint returns 401.
- tc_auth_g_disabled_user_token_401 · function · L229-L252 — Verifies that disabling a user cascades session invalidation so their previously-valid token returns 401.
