---
name: Code Generation
slug: code-generation
type: system
sources:
  - path: src/codegen.rs
    hash: 3ccf64715b881949fdcc051ffded10833daa895820cfb753ec66aae6aba3a6ec
sources_digest: 9771858fa68359c846f6be02025855ddf2c0d334f60bc5c1dc259cf889687d8e
links:
  - to: ontology-ir-odl-parsing
    relation: uses
    description: Consumes OntologyIR and ObjectType from ir.rs.
  - to: persistence-layer
    relation: produces
    description: >-
      The canonical_schema DDL string is the schema LibSQLStorage creates and
      validates against.
generator:
  version: 1
covers:
  - symbol: CodegenError
    kind: struct
    at: 'src/codegen.rs:L12-L15'
  - symbol: new
    kind: function
    at: 'src/codegen.rs:L18-L23'
  - symbol: CodegenOutput
    kind: struct
    at: 'src/codegen.rs:L28-L35'
  - symbol: ObjectDef
    kind: struct
    at: 'src/codegen.rs:L39-L42'
  - symbol: codegen
    kind: function
    at: 'src/codegen.rs:L45-L59'
  - symbol: gen_struct
    kind: function
    at: 'src/codegen.rs:L62-L81'
  - symbol: canonical_schema
    kind: function
    at: 'src/codegen.rs:L84-L250'
  - symbol: to_json
    kind: function
    at: 'src/codegen.rs:L253-L255'
  - symbol: roundtrip_ok
    kind: function
    at: 'src/codegen.rs:L258-L263'
  - symbol: _assert_serialize
    kind: function
    at: 'src/codegen.rs:L267-L267'
---
<!-- context:generated:start -->
## Summary

Transforms OntologyIR into three artifacts: Rust structs (with embedded JSON `ObjectDef` constants enabling runtime-driven behavior without match branches), a canonical SQL DDL string (deliberately avoiding per-type tables), and JSON serialization. The canonical schema includes `wb_meta.schema_version` to guard against stale databases, and `roundtrip_ok` verifies serialization fidelity.

## Related

- uses [[ontology-ir-odl-parsing]] — Consumes OntologyIR and ObjectType from ir.rs.
- produces [[persistence-layer]] — The canonical_schema DDL string is the schema LibSQLStorage creates and validates against.
<!-- context:generated:end -->

## Notes

_Anything written below the generated block is preserved when the graph is regenerated._
