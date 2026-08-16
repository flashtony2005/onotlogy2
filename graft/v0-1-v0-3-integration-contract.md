---
name: V0.1–V0.3 Integration Contract
slug: v0-1-v0-3-integration-contract
type: system
sources:
  - path: tests/v0_1.rs
    hash: 2bf0838233b72b77468f7f936fc325cf82d469c91e1a0aeced877881f2f11442
  - path: tests/v0_2.rs
    hash: 39757773749ee020a794fed76465fcad36e3718f2705eb2c076281e853e6149b
  - path: tests/v0_3.rs
    hash: 36a45bc6af635916871b833f07de61d530477a4f6b3836010a92692759ccc5d2
sources_digest: 66d8abb9d802da1b0585e75d6a1c0870af74dcdc4b9946c66c189178f32a7d0e
links:
  - to: ontology-ir-odl-parsing
    relation: validates
    description: Asserts parse_odl error codes and IR fidelity.
  - to: persistence-layer
    relation: validates
    description: 'Tests CRUD, versioning, WB2101 stale-schema rejection.'
  - to: planner-policy-engine
    relation: validates
    description: 'Drives Planner with FakeEngine, tests policy enforcement and approvals.'
  - to: runtime-kernel-observability
    relation: validates
    description: 'Verifies trace events, object_refs, event bus consistency.'
generator:
  version: 1
covers:
  - symbol: tc001_parse_valid
    kind: function
    at: 'tests/v0_1.rs:L13-L26'
  - symbol: tc001b_parse_invalid
    kind: function
    at: 'tests/v0_1.rs:L29-L41'
  - symbol: tc002_ir_fidelity
    kind: function
    at: 'tests/v0_1.rs:L45-L52'
  - symbol: tc003_codegen
    kind: function
    at: 'tests/v0_1.rs:L56-L69'
  - symbol: tc004_object_crud
    kind: function
    at: 'tests/v0_1.rs:L73-L92'
  - symbol: tc005_turso_roundtrip
    kind: function
    at: 'tests/v0_1.rs:L96-L109'
  - symbol: tc006_render_list
    kind: function
    at: 'tests/v0_1.rs:L113-L130'
  - symbol: tc007_world_trace
    kind: function
    at: 'tests/v0_1.rs:L135-L166'
  - symbol: tc007b_missing_refs_blocked
    kind: function
    at: 'tests/v0_1.rs:L169-L189'
  - symbol: tc005b_open_rejects_legacy_schema
    kind: function
    at: 'tests/v0_1.rs:L193-L221'
  - symbol: tc012_relation_link_and_neighbors
    kind: function
    at: 'tests/v0_2.rs:L10-L36'
  - symbol: tc012b_duplicate_edge_rejected
    kind: function
    at: 'tests/v0_2.rs:L40-L61'
  - symbol: tc013_domain_event_emitted
    kind: function
    at: 'tests/v0_2.rs:L65-L86'
---
<!-- context:generated:start -->
## Summary

The integration test suites locking the core pipeline contracts: ODL parsing error codes (1001/1002/1005), IR roundtrip fidelity, storage CRUD with version increments, runtime tracing with object_refs, event bus consistency, relation linking with WB2007 duplicate rejection, domain events, planner-driven runs with FakeEngine/UnavailableEngine, three-tier view filtering, WS reconnection, and REST error codes (WB1001/WB2002/WB2004/WB2005). TC-005-b verifies legacy schema files return WB2101 rather than cryptic failures.

## Related

- validates [[ontology-ir-odl-parsing]] — Asserts parse_odl error codes and IR fidelity.
- validates [[persistence-layer]] — Tests CRUD, versioning, WB2101 stale-schema rejection.
- validates [[planner-policy-engine]] — Drives Planner with FakeEngine, tests policy enforcement and approvals.
- validates [[runtime-kernel-observability]] — Verifies trace events, object_refs, event bus consistency.
<!-- context:generated:end -->

## Notes

_Anything written below the generated block is preserved when the graph is regenerated._
