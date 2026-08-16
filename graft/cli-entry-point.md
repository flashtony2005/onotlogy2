---
name: CLI Entry Point
slug: cli-entry-point
type: system
sources:
  - path: src/bin/oaip.rs
    hash: 46385ea7680f656020db69e4572168a44b3e0bd0807e6d16eb406622ed783ab4
sources_digest: 02ffa2e96daf7d46f8e4e86bc6ae1b63a8c1173a80cdf9a9e0b6c5a50ea9d4e2
links:
  - to: planner-policy-engine
    relation: configures
    description: Selects PolicyApprover (Auto/Console) and auto_approve_evolution flag.
  - to: server-api
    relation: configures
    description: serve command boots the server with auth and policy approvers.
generator:
  version: 1
covers: []
---
<!-- context:generated:start -->
## Summary

The `oaip` binary wiring all modules: ODL compilation, object/relation/event management, agent runs, observation UI serving, and user administration. Enforces V0.7 auth on `serve` requiring a bootstrap admin. Notable choices: legacy fallback to empty ActionRegistry without an ODL file, `auto_approve_evolution` flag for REQ-018, and `db reset` deleting SQLite + WAL/SHM companions before recreating schema.

## Related

- configures [[planner-policy-engine]] — Selects PolicyApprover (Auto/Console) and auto_approve_evolution flag.
- configures [[server-api]] — serve command boots the server with auth and policy approvers.
<!-- context:generated:end -->

## Notes

_Anything written below the generated block is preserved when the graph is regenerated._
