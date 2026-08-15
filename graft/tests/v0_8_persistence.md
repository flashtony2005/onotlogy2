# tests/v0_8_persistence.rs · [[auth-persistence-integration-tests]]

Integration test suite verifying v0.7 runtime persistence across restart/crash scenarios (TC-persist a-d), covering run lifecycle, restart rebuild, crash-to-Interrupted, and orphan Pending-to-Expired recovery.

- FakeEngine · struct · L26-L26 — Minimal test agent engine that returns fixed decisions per step to drive the legacy retention loop without an approval gate.
- name · function · L30-L32 — Returns the engine identifier 'fake' for the test agent.
- reason · function · L33-L55 — Produces a deterministic Decision for each workflow step, mapping known step names to retention-loop intents and collecting object refs from the world state.
- spawn_server · function · L59-L79 — Boots a server on an ephemeral port reusing the caller's kernel storage connection so persistence can be verified across restarts.
- http_call · function · L82-L117 — Performs a raw HTTP request against the test server and returns status plus body, trimming the body to Content-Length.
- wait_run_done · function · L120-L130 — Polls the storage run record until it leaves Running state (terminal) or times out, returning the final record.
- temp_db · function · L133-L141 — Generates a unique temporary file-db path per process/run for isolated persistence tests.
- tc_persist_a_run_lifecycle_persisted · function · L145-L179 — Verifies a triggered run is inserted into wb_run as Running immediately and transitions to Completed in the DB as the source of truth.
- tc_persist_b_restart_rebuilds_run_map · function · L183-L239 — Verifies that after restarting with the same file db, the timeline rebuilds from DB and reports Completed consistently with storage.
- tc_persist_c_crash_running_to_interrupted · function · L243-L278 — Verifies that a Running run left behind by a crash is marked Interrupted when the server restarts and runs recovery.
- tc_persist_d_orphan_pending_to_expired · function · L282-L320 — Verifies that an orphaned Pending approval with no live run is flipped to Expired on server restart recovery.
