---
name: Planner & Policy Engine
slug: planner-policy-engine
type: system
sources:
  - path: src/planner.rs
    hash: 6731138539e062f8a3b9ca2b639536609861c30ea54fac8d5ffed8ea0d5f9a13
sources_digest: 14405aef52e43915a9cd6adfd524228aa7de7bf8491fc573d9d2119c8da973ae
links:
  - to: action-engine
    relation: uses
    description: Executes approved plan steps via ActionEngine/ActionRegistry.
  - to: cognitive-engine-pi
    relation: uses
    description: >-
      Calls AgentEngine to get Decisions; Pi is strictly cognitive, Planner owns
      decisions.
  - to: runtime-kernel-observability
    relation: uses
    description: Persists approval audits and run state through RuntimeKernel.
generator:
  version: 1
covers:
  - symbol: APPROVAL_TIMEOUT
    kind: constant
    at: 'src/planner.rs:L26-L26'
  - symbol: RuntimeConfig
    kind: struct
    at: 'src/planner.rs:L31-L34'
  - symbol: ApprovalAuditor
    kind: interface
    at: 'src/planner.rs:L39-L41'
  - symbol: record
    kind: function
    at: 'src/planner.rs:L40-L40'
  - symbol: may_auto_approve
    kind: function
    at: 'src/planner.rs:L45-L85'
  - symbol: AUTO_APPROVE_CONFIDENCE
    kind: constant
    at: 'src/planner.rs:L53-L53'
  - symbol: AUTO_APPROVE_MIN_RUNS
    kind: constant
    at: 'src/planner.rs:L55-L55'
  - symbol: PlannerError
    kind: enum
    at: 'src/planner.rs:L89-L101'
  - symbol: RunStatus
    kind: enum
    at: 'src/planner.rs:L105-L111'
  - symbol: as_str
    kind: function
    at: 'src/planner.rs:L114-L121'
  - symbol: from_db_str
    kind: function
    at: 'src/planner.rs:L124-L131'
  - symbol: AgentRunResult
    kind: struct
    at: 'src/planner.rs:L136-L142'
  - symbol: Capability
    kind: enum
    at: 'src/planner.rs:L146-L175'
  - symbol: PlanStep
    kind: struct
    at: 'src/planner.rs:L179-L183'
  - symbol: Policy
    kind: struct
    at: 'src/planner.rs:L187-L187'
  - symbol: PolicyApprover
    kind: interface
    at: 'src/planner.rs:L195-L198'
  - symbol: approve
    kind: function
    at: 'src/planner.rs:L197-L197'
  - symbol: AutoApprover
    kind: struct
    at: 'src/planner.rs:L202-L204'
  - symbol: approve
    kind: function
    at: 'src/planner.rs:L207-L209'
  - symbol: ConsoleApprover
    kind: struct
    at: 'src/planner.rs:L216-L218'
  - symbol: new
    kind: function
    at: 'src/planner.rs:L220-L224'
  - symbol: key_of
    kind: function
    at: 'src/planner.rs:L227-L234'
  - symbol: default
    kind: function
    at: 'src/planner.rs:L237-L239'
  - symbol: approve
    kind: function
    at: 'src/planner.rs:L243-L254'
  - symbol: prompt
    kind: function
    at: 'src/planner.rs:L259-L267'
  - symbol: RunAuditor
    kind: struct
    at: 'src/planner.rs:L271-L274'
  - symbol: new
    kind: function
    at: 'src/planner.rs:L276-L281'
  - symbol: record
    kind: function
    at: 'src/planner.rs:L285-L296'
  - symbol: ApprovalState
    kind: enum
    at: 'src/planner.rs:L302-L308'
  - symbol: as_str
    kind: function
    at: 'src/planner.rs:L312-L319'
  - symbol: from_db_str
    kind: function
    at: 'src/planner.rs:L322-L329'
  - symbol: ApprovalRequest
    kind: struct
    at: 'src/planner.rs:L335-L343'
  - symbol: ApprovalStore
    kind: struct
    at: 'src/planner.rs:L346-L348'
  - symbol: ApprovalEntry
    kind: struct
    at: 'src/planner.rs:L350-L354'
  - symbol: new
    kind: function
    at: 'src/planner.rs:L357-L361'
  - symbol: list_for_run
    kind: function
    at: 'src/planner.rs:L364-L372'
  - symbol: get
    kind: function
    at: 'src/planner.rs:L375-L381'
  - symbol: register
    kind: function
    at: 'src/planner.rs:L384-L394'
  - symbol: expire
    kind: function
    at: 'src/planner.rs:L400-L408'
  - symbol: requests_insert_for_test
    kind: function
    at: 'src/planner.rs:L413-L434'
  - symbol: resolve
    kind: function
    at: 'src/planner.rs:L438-L454'
  - symbol: default
    kind: function
    at: 'src/planner.rs:L458-L460'
  - symbol: ServerApprover
    kind: struct
    at: 'src/planner.rs:L466-L474'
  - symbol: new
    kind: function
    at: 'src/planner.rs:L477-L491'
  - symbol: with_timeout
    kind: function
    at: 'src/planner.rs:L494-L497'
  - symbol: approve
    kind: function
    at: 'src/planner.rs:L502-L582'
  - symbol: check
    kind: function
    at: 'src/planner.rs:L587-L594'
  - symbol: check_action
    kind: function
    at: 'src/planner.rs:L604-L655'
  - symbol: resolve_policy_field
    kind: function
    at: 'src/planner.rs:L659-L672'
  - symbol: resolve_value
    kind: function
    at: 'src/planner.rs:L675-L690'
  - symbol: compare_values
    kind: function
    at: 'src/planner.rs:L693-L709'
  - symbol: PolicyDecision
    kind: enum
    at: 'src/planner.rs:L713-L716'
  - symbol: LLMGoalParser
    kind: struct
    at: 'src/planner.rs:L719-L719'
  - symbol: parse
    kind: function
    at: 'src/planner.rs:L723-L746'
  - symbol: Goal
    kind: struct
    at: 'src/planner.rs:L751-L755'
  - symbol: Planner
    kind: struct
    at: 'src/planner.rs:L758-L766'
  - symbol: new
    kind: function
    at: 'src/planner.rs:L770-L777'
  - symbol: with_actions
    kind: function
    at: 'src/planner.rs:L780-L787'
  - symbol: with_config
    kind: function
    at: 'src/planner.rs:L790-L793'
  - symbol: with_approver
    kind: function
    at: 'src/planner.rs:L796-L799'
  - symbol: select_candidates
    kind: function
    at: 'src/planner.rs:L803-L817'
  - symbol: build_plan
    kind: function
    at: 'src/planner.rs:L821-L847'
  - symbol: run_goal
    kind: function
    at: 'src/planner.rs:L851-L1116'
  - symbol: reason
    kind: function
    at: 'src/planner.rs:L1120-L1139'
  - symbol: execute_step
    kind: function
    at: 'src/planner.rs:L1142-L1235'
  - symbol: build_retention_plan
    kind: function
    at: 'src/planner.rs:L1239-L1263'
  - symbol: call_engine
    kind: function
    at: 'src/planner.rs:L1266-L1291'
  - symbol: non_empty
    kind: function
    at: 'src/planner.rs:L1294-L1300'
  - symbol: new_run_id
    kind: function
    at: 'src/planner.rs:L1303-L1310'
  - symbol: RUN_SEQ
    kind: constant
    at: 'src/planner.rs:L1304-L1304'
  - symbol: generate_run_id
    kind: function
    at: 'src/planner.rs:L1313-L1315'
---
<!-- context:generated:start -->
## Summary

The orchestration engine running Goal → LLMGoalParser → Planner → PlanStep → Capability → Execute. Coordinates the cognitive engine with action execution, enforcing policy preconditions and approval gates. Implements REQ-018's five-gate `may_auto_approve` pure function, three PolicyApprover implementations (Auto/Console/Server), and an ApprovalStore with oneshot channels for pending approvals. Default-deny posture for destructive operations; auto-approval always writes an audit record first.

## Related

- uses [[action-engine]] — Executes approved plan steps via ActionEngine/ActionRegistry.
- uses [[cognitive-engine-pi]] — Calls AgentEngine to get Decisions; Pi is strictly cognitive, Planner owns decisions.
- uses [[runtime-kernel-observability]] — Persists approval audits and run state through RuntimeKernel.
<!-- context:generated:end -->

## Notes

_Anything written below the generated block is preserved when the graph is regenerated._
