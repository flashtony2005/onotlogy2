---
name: Learn≠Permission Invariant
slug: learn-permission-invariant
type: concept
sources:
  - path: src/evolution.rs
    hash: 8b22e1db63cf8cb31cf6cdc3f9664b067373f58a2c20046e9cfc21657c44cef0
  - path: src/planner.rs
    hash: 6731138539e062f8a3b9ca2b639536609861c30ea54fac8d5ffed8ea0d5f9a13
  - path: src/runtime.rs
    hash: 5f2cab0b89c4304f864d96ccf1b2be11dc741f2d472d3eae69974bfe62cec174
  - path: ui/src/api.ts
    hash: 9877ae14f5d95f9c195b0c746714ec0a4efc5fa91ae8f5435a5fcf9ce14fd474
  - path: ui/src/App.tsx
    hash: e18a74c3f078e5499cfa589ad4362aec60dc2bfde351545f3689eb7d75f226d6
sources_digest: b6b660a91644fa18eeef20d1cfe218708561a1b54b7ff3e7b14b59f55b1c29d5
links:
  - to: evolution-engine
    relation: implements
    description: The pipeline enforces the separation of knowledge from permissions.
  - to: planner-policy-engine
    relation: implements
    description: may_auto_approve gates and audit-before-execute enforce the invariant.
generator:
  version: 1
covers:
  - symbol: PATTERN_MIN_HITS
    kind: constant
    at: 'src/evolution.rs:L27-L27'
  - symbol: CONFIDENCE_DIVISOR
    kind: constant
    at: 'src/evolution.rs:L29-L29'
  - symbol: VALIDATE_THRESHOLD
    kind: constant
    at: 'src/evolution.rs:L31-L31'
  - symbol: pattern_label_of
    kind: function
    at: 'src/evolution.rs:L35-L37'
  - symbol: proposal_id_of
    kind: function
    at: 'src/evolution.rs:L40-L42'
  - symbol: learned_id_of
    kind: function
    at: 'src/evolution.rs:L45-L47'
  - symbol: Experience
    kind: struct
    at: 'src/evolution.rs:L52-L55'
  - symbol: action
    kind: function
    at: 'src/evolution.rs:L59-L64'
  - symbol: Pattern
    kind: struct
    at: 'src/evolution.rs:L69-L80'
  - symbol: confidence_of
    kind: function
    at: 'src/evolution.rs:L87-L89'
  - symbol: ProposalKind
    kind: enum
    at: 'src/evolution.rs:L94-L96'
  - symbol: ProposalStatus
    kind: enum
    at: 'src/evolution.rs:L100-L104'
  - symbol: Proposal
    kind: struct
    at: 'src/evolution.rs:L109-L121'
  - symbol: LearnedKnowledge
    kind: struct
    at: 'src/evolution.rs:L126-L141'
  - symbol: default_true
    kind: function
    at: 'src/evolution.rs:L143-L145'
  - symbol: EvolutionState
    kind: struct
    at: 'src/evolution.rs:L149-L153'
  - symbol: mine_patterns
    kind: function
    at: 'src/evolution.rs:L159-L179'
  - symbol: propose
    kind: function
    at: 'src/evolution.rs:L182-L192'
  - symbol: validate
    kind: function
    at: 'src/evolution.rs:L199-L209'
  - symbol: parse_run_ids
    kind: function
    at: 'src/evolution.rs:L212-L214'
  - symbol: encode_run_ids
    kind: function
    at: 'src/evolution.rs:L217-L224'
  - symbol: merge_run_ids
    kind: function
    at: 'src/evolution.rs:L227-L234'
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
  - symbol: TraceKind
    kind: enum
    at: 'src/runtime.rs:L19-L22'
  - symbol: as_str
    kind: function
    at: 'src/runtime.rs:L25-L30'
  - symbol: Err
    kind: type
    at: 'src/runtime.rs:L34-L34'
  - symbol: from_str
    kind: function
    at: 'src/runtime.rs:L35-L41'
  - symbol: TraceEvent
    kind: struct
    at: 'src/runtime.rs:L46-L59'
  - symbol: TRACE_SEQ
    kind: constant
    at: 'src/runtime.rs:L61-L61'
  - symbol: now_ts
    kind: function
    at: 'src/runtime.rs:L63-L68'
  - symbol: new_trace_id
    kind: function
    at: 'src/runtime.rs:L70-L76'
  - symbol: world
    kind: function
    at: 'src/runtime.rs:L80-L82'
  - symbol: world_with_refs
    kind: function
    at: 'src/runtime.rs:L86-L100'
  - symbol: EventBus
    kind: struct
    at: 'src/runtime.rs:L104-L106'
  - symbol: default
    kind: function
    at: 'src/runtime.rs:L109-L111'
  - symbol: new
    kind: function
    at: 'src/runtime.rs:L115-L118'
  - symbol: publish
    kind: function
    at: 'src/runtime.rs:L120-L122'
  - symbol: subscribe
    kind: function
    at: 'src/runtime.rs:L124-L126'
  - symbol: RuntimeKernel
    kind: struct
    at: 'src/runtime.rs:L131-L134'
  - symbol: new
    kind: function
    at: 'src/runtime.rs:L137-L142'
  - symbol: event_bus
    kind: function
    at: 'src/runtime.rs:L144-L146'
  - symbol: storage
    kind: function
    at: 'src/runtime.rs:L150-L152'
  - symbol: create_object
    kind: function
    at: 'src/runtime.rs:L154-L163'
  - symbol: update_object
    kind: function
    at: 'src/runtime.rs:L165-L170'
  - symbol: get_object
    kind: function
    at: 'src/runtime.rs:L173-L175'
  - symbol: delete_object
    kind: function
    at: 'src/runtime.rs:L177-L181'
  - symbol: emit
    kind: function
    at: 'src/runtime.rs:L184-L188'
  - symbol: emit_cognitive
    kind: function
    at: 'src/runtime.rs:L193-L218'
  - symbol: list_events
    kind: function
    at: 'src/runtime.rs:L221-L223'
  - symbol: get_trace
    kind: function
    at: 'src/runtime.rs:L226-L228'
  - symbol: list_world_events
    kind: function
    at: 'src/runtime.rs:L231-L233'
  - symbol: snapshot_world
    kind: function
    at: 'src/runtime.rs:L236-L248'
  - symbol: query_objects
    kind: function
    at: 'src/runtime.rs:L251-L256'
  - symbol: link_relation
    kind: function
    at: 'src/runtime.rs:L261-L277'
  - symbol: neighbors
    kind: function
    at: 'src/runtime.rs:L280-L282'
  - symbol: query_relations
    kind: function
    at: 'src/runtime.rs:L285-L290'
  - symbol: emit_domain_event
    kind: function
    at: 'src/runtime.rs:L293-L301'
  - symbol: record_memory
    kind: function
    at: 'src/runtime.rs:L306-L312'
  - symbol: recall_memory
    kind: function
    at: 'src/runtime.rs:L315-L320'
  - symbol: recall_all
    kind: function
    at: 'src/runtime.rs:L323-L325'
  - symbol: run_evolution
    kind: function
    at: 'src/runtime.rs:L334-L394'
  - symbol: evolution_state
    kind: function
    at: 'src/runtime.rs:L397-L403'
  - symbol: revoke_learned
    kind: function
    at: 'src/runtime.rs:L406-L408'
  - symbol: save_learned
    kind: function
    at: 'src/runtime.rs:L413-L418'
  - symbol: set_learned_auto_approve
    kind: function
    at: 'src/runtime.rs:L422-L428'
  - symbol: record_approval_audit
    kind: function
    at: 'src/runtime.rs:L431-L442'
  - symbol: list_approval_audit
    kind: function
    at: 'src/runtime.rs:L445-L450'
  - symbol: count_action_failed
    kind: function
    at: 'src/runtime.rs:L453-L455'
  - symbol: fmtTime
    kind: function
    at: 'ui/src/App.tsx:L33-L37'
  - symbol: fmtJson
    kind: function
    at: 'ui/src/App.tsx:L39-L47'
  - symbol: App
    kind: function
    at: 'ui/src/App.tsx:L49-L273'
  - symbol: reloadMemory
    kind: function
    at: 'ui/src/App.tsx:L62-L72'
  - symbol: reloadEvolution
    kind: function
    at: 'ui/src/App.tsx:L78-L88'
  - symbol: startRun
    kind: function
    at: 'ui/src/App.tsx:L106-L123'
  - symbol: MemoryPanel
    kind: function
    at: 'ui/src/App.tsx:L276-L316'
  - symbol: EvolutionPanel
    kind: function
    at: 'ui/src/App.tsx:L319-L466'
  - symbol: toggleAutoApprove
    kind: function
    at: 'ui/src/App.tsx:L337-L347'
  - symbol: showAudit
    kind: function
    at: 'ui/src/App.tsx:L349-L352'
  - symbol: unwrap
    kind: function
    at: 'ui/src/api.ts:L13-L19'
  - symbol: postRun
    kind: function
    at: 'ui/src/api.ts:L21-L28'
  - symbol: getTimeline
    kind: function
    at: 'ui/src/api.ts:L30-L35'
  - symbol: getMemory
    kind: function
    at: 'ui/src/api.ts:L38-L41'
  - symbol: getEvolution
    kind: function
    at: 'ui/src/api.ts:L45-L48'
  - symbol: postAutoApproveToggle
    kind: function
    at: 'ui/src/api.ts:L52-L62'
  - symbol: getApprovalAudit
    kind: function
    at: 'ui/src/api.ts:L65-L68'
---
<!-- context:generated:start -->
## Summary

A cross-cutting design invariant: the evolution engine may convert experiences into validated knowledge but must never modify policies or bypass human approval. Enforced at multiple layers — evolution.rs separates knowledge validation from permission changes, the UI keeps evolution read-only except explicit auto-approval toggles, and auto-approval always writes an audit record before execution. The revocable auto_approve_enabled flag defaults to true but can force human approval.

## Related

- implements [[evolution-engine]] — The pipeline enforces the separation of knowledge from permissions.
- implements [[planner-policy-engine]] — may_auto_approve gates and audit-before-execute enforce the invariant.
<!-- context:generated:end -->

## Notes

_Anything written below the generated block is preserved when the graph is regenerated._
