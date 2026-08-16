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
    at: 'src/bin/oaip.rs:L19-L24'
  - symbol: Cmd
    kind: enum
    at: 'src/bin/oaip.rs:L27-L104'
  - symbol: AgentCmd
    kind: enum
    at: 'src/bin/oaip.rs:L107-L131'
  - symbol: WorkerCmd
    kind: enum
    at: 'src/bin/oaip.rs:L134-L154'
  - symbol: ObjectCmd
    kind: enum
    at: 'src/bin/oaip.rs:L157-L160'
  - symbol: RelationCmd
    kind: enum
    at: 'src/bin/oaip.rs:L163-L177'
  - symbol: EventCmd
    kind: enum
    at: 'src/bin/oaip.rs:L180-L189'
  - symbol: DbCmd
    kind: enum
    at: 'src/bin/oaip.rs:L192-L195'
  - symbol: UserCmd
    kind: enum
    at: 'src/bin/oaip.rs:L198-L221'
  - symbol: main
    kind: function
    at: 'src/bin/oaip.rs:L224-L333'
  - symbol: cmd_compile
    kind: function
    at: 'src/bin/oaip.rs:L335-L348'
  - symbol: cmd_run
    kind: function
    at: 'src/bin/oaip.rs:L350-L364'
  - symbol: cmd_object_list
    kind: function
    at: 'src/bin/oaip.rs:L366-L375'
  - symbol: cmd_event_list
    kind: function
    at: 'src/bin/oaip.rs:L377-L391'
  - symbol: cmd_relation_link
    kind: function
    at: 'src/bin/oaip.rs:L393-L408'
  - symbol: cmd_relation_list
    kind: function
    at: 'src/bin/oaip.rs:L410-L432'
  - symbol: cmd_event_emit
    kind: function
    at: 'src/bin/oaip.rs:L434-L449'
  - symbol: cmd_db_reset
    kind: function
    at: 'src/bin/oaip.rs:L451-L462'
  - symbol: cmd_agent_run
    kind: function
    at: 'src/bin/oaip.rs:L467-L755'
  - symbol: default_worker_env
    kind: function
    at: 'src/bin/oaip.rs:L759-L785'
  - symbol: policy_for_capabilities
    kind: function
    at: 'src/bin/oaip.rs:L788-L798'
  - symbol: cmd_worker_run
    kind: function
    at: 'src/bin/oaip.rs:L802-L916'
  - symbol: cmd_replay
    kind: function
    at: 'src/bin/oaip.rs:L919-L940'
  - symbol: cmd_simulate
    kind: function
    at: 'src/bin/oaip.rs:L944-L964'
  - symbol: simulate_inner
    kind: function
    at: 'src/bin/oaip.rs:L966-L1064'
  - symbol: cmd_health
    kind: function
    at: 'src/bin/oaip.rs:L1067-L1094'
  - symbol: cmd_serve
    kind: function
    at: 'src/bin/oaip.rs:L1097-L1151'
  - symbol: cmd_user_create_admin
    kind: function
    at: 'src/bin/oaip.rs:L1154-L1164'
  - symbol: cmd_user_create
    kind: function
    at: 'src/bin/oaip.rs:L1167-L1191'
  - symbol: cmd_user_list
    kind: function
    at: 'src/bin/oaip.rs:L1194-L1208'
  - symbol: cmd_user_delete
    kind: function
    at: 'src/bin/oaip.rs:L1211-L1216'
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
