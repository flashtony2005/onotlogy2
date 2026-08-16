# tests/v0_7_auth.rs · [[auth-persistence-integration-tests]]

Integration test contract for V0.7 auth & RBAC: verifies server-enforced authentication (401 on missing/wrong token), role capability gating (403 for insufficient role), view clamping to role ceiling, and login/logout/disable lifecycle.

- common · module · L6-L6 — mod common;
- login · function · L19-L28 — Performs a login POST and extracts the token from the JSON response when status is 200.
- spawn_auth_server · function · L32-L66 — Boots an in-memory auth-enforced server with admin(Developer) and exec(Executive) users, returning the address and shared storage for direct account manipulation.
- tc_auth_a_no_token_401 · function · L71-L82 — Verifies that accessing a protected agent/run endpoint without a token returns 401.
- tc_auth_b_wrong_token_401 · function · L86-L97 — Verifies that a malformed/invalid Bearer token is rejected with 401.
- tc_auth_c_executive_run_forbidden · function · L101-L114 — Verifies that an Executive-role user is denied (403) from triggering agent runs which require Operator or above.
- tc_auth_d_view_clamped_to_role · function · L118-L139 — Verifies the server clamps a frontend-requested ?view=developer down to the Executive role ceiling and strips reasoning/input/output detail from events.
- tc_auth_e_login_ok_and_token_works · function · L143-L152 — Verifies a valid admin login returns 200 with a token and that the token grants access to /auth/me reporting role developer.
- tc_auth_f_logout_invalidates_token · function · L156-L165 — Verifies that after logout, reusing the same token on a protected endpoint returns 401.
- tc_auth_g_disabled_user_token_401 · function · L169-L192 — Verifies that disabling a user cascades session invalidation so their previously-valid token returns 401.
- tc_auth_h_graph_clamped_to_role · function · L197-L231 — async fn tc_auth_h_graph_clamped_to_role()
- tc_auth_i_trace_list_clamped_to_role · function · L236-L294 — async fn tc_auth_i_trace_list_clamped_to_role()
- tc_auth_j_login_rate_limited · function · L298-L309 — async fn tc_auth_j_login_rate_limited()
- tc_auth_k_session_token_hashed_at_rest · function · L313-L323 — async fn tc_auth_k_session_token_hashed_at_rest()
