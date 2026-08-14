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
covers:
  - symbol: Cli
    kind: struct
    at: 'src/bin/oaip.rs:L22-L27'
  - symbol: Cmd
    kind: enum
    at: 'src/bin/oaip.rs:L30-L83'
  - symbol: AgentCmd
    kind: enum
    at: 'src/bin/oaip.rs:L86-L104'
  - symbol: ObjectCmd
    kind: enum
    at: 'src/bin/oaip.rs:L107-L110'
  - symbol: RelationCmd
    kind: enum
    at: 'src/bin/oaip.rs:L113-L127'
  - symbol: EventCmd
    kind: enum
    at: 'src/bin/oaip.rs:L130-L139'
  - symbol: DbCmd
    kind: enum
    at: 'src/bin/oaip.rs:L142-L145'
  - symbol: UserCmd
    kind: enum
    at: 'src/bin/oaip.rs:L148-L156'
  - symbol: main
    kind: function
    at: 'src/bin/oaip.rs:L159-L229'
  - symbol: cmd_compile
    kind: function
    at: 'src/bin/oaip.rs:L231-L244'
  - symbol: cmd_run
    kind: function
    at: 'src/bin/oaip.rs:L246-L261'
  - symbol: cmd_object_list
    kind: function
    at: 'src/bin/oaip.rs:L263-L272'
  - symbol: cmd_event_list
    kind: function
    at: 'src/bin/oaip.rs:L274-L288'
  - symbol: cmd_relation_link
    kind: function
    at: 'src/bin/oaip.rs:L290-L306'
  - symbol: cmd_relation_list
    kind: function
    at: 'src/bin/oaip.rs:L308-L331'
  - symbol: cmd_event_emit
    kind: function
    at: 'src/bin/oaip.rs:L333-L346'
  - symbol: cmd_db_reset
    kind: function
    at: 'src/bin/oaip.rs:L348-L359'
  - symbol: cmd_agent_run
    kind: function
    at: 'src/bin/oaip.rs:L362-L429'
  - symbol: cmd_serve
    kind: function
    at: 'src/bin/oaip.rs:L432-L480'
  - symbol: cmd_user_create_admin
    kind: function
    at: 'src/bin/oaip.rs:L483-L493'
  - symbol: load_registry
    kind: function
    at: 'src/bin/oaip.rs:L496-L505'
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
