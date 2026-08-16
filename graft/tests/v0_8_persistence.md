# tests/v0_8_persistence.rs · [[auth-persistence-integration-tests]]

Integration test suite verifying v0.7 runtime persistence across restart/crash scenarios (TC-persist a-d), covering run lifecycle, restart rebuild, crash-to-Interrupted, and orphan Pending-to-Expired recovery.

- common · module · L10-L10 — mod common;
- wait_run_done · function · L24-L34 — Polls the storage run record until it leaves Running state (terminal) or times out, returning the final record.
- temp_db · function · L37-L45 — Generates a unique temporary file-db path per process/run for isolated persistence tests.
- tc_persist_a_run_lifecycle_persisted · function · L50-L82 — Verifies a triggered run is inserted into wb_run as Running immediately and transitions to Completed in the DB as the source of truth.
- tc_persist_b_restart_rebuilds_run_map · function · L86-L132 — Verifies that after restarting with the same file db, the timeline rebuilds from DB and reports Completed consistently with storage.
- tc_persist_c_crash_running_to_interrupted · function · L136-L157 — Verifies that a Running run left behind by a crash is marked Interrupted when the server restarts and runs recovery.
