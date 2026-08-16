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
covers: []
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
