---
name: Ontology IR & ODL Parsing
slug: ontology-ir-odl-parsing
type: system
sources:
  - path: src/ir.rs
    hash: 450ff5fd7028f0c4fcb9bdbbeda3fa6c93b7c555934b5eb19d2838a598e4647f
  - path: src/odl.rs
    hash: 234949cb0e91f44545f73701498000fb405d678202f2ce51b583506d75f2d169
sources_digest: f29249acf6c660265e2bf0610f1d20b72925fe8d520ab5dd98aa12cda6a4c5b2
links:
  - to: action-engine
    relation: produces
    description: ActionRegistry builds its type-to-action map from OntologyIR ActionDefs.
  - to: code-generation
    relation: produces
    description: 'codegen consumes OntologyIR to emit Rust structs, SQL schema, and JSON.'
generator:
  version: 1
covers:
  - symbol: ScalarType
    kind: enum
    at: 'src/ir.rs:L9-L14'
  - symbol: rust_type
    kind: function
    at: 'src/ir.rs:L18-L25'
  - symbol: sqlite_type
    kind: function
    at: 'src/ir.rs:L28-L35'
  - symbol: Err
    kind: type
    at: 'src/ir.rs:L39-L39'
  - symbol: from_str
    kind: function
    at: 'src/ir.rs:L40-L48'
  - symbol: fmt
    kind: function
    at: 'src/ir.rs:L52-L60'
  - symbol: PropertyDef
    kind: struct
    at: 'src/ir.rs:L65-L68'
  - symbol: ParamDef
    kind: struct
    at: 'src/ir.rs:L72-L75'
  - symbol: ValueExpr
    kind: enum
    at: 'src/ir.rs:L81-L90'
  - symbol: FieldUpdate
    kind: struct
    at: 'src/ir.rs:L94-L97'
  - symbol: CreateEffect
    kind: struct
    at: 'src/ir.rs:L101-L113'
  - symbol: CompareOp
    kind: enum
    at: 'src/ir.rs:L117-L124'
  - symbol: Clause
    kind: struct
    at: 'src/ir.rs:L130-L134'
  - symbol: PolicyDef
    kind: struct
    at: 'src/ir.rs:L138-L145'
  - symbol: ActionDef
    kind: struct
    at: 'src/ir.rs:L153-L164'
  - symbol: ObjectType
    kind: struct
    at: 'src/ir.rs:L168-L174'
  - symbol: OntologyIR
    kind: struct
    at: 'src/ir.rs:L178-L180'
  - symbol: find
    kind: function
    at: 'src/ir.rs:L184-L186'
  - symbol: OdlError
    kind: struct
    at: 'src/odl.rs:L37-L42'
  - symbol: new
    kind: function
    at: 'src/odl.rs:L45-L52'
  - symbol: E_OBJECT_NAME
    kind: constant
    at: 'src/odl.rs:L56-L56'
  - symbol: E_DUP_OBJECT
    kind: constant
    at: 'src/odl.rs:L57-L57'
  - symbol: E_NO_PROP_BLOCK
    kind: constant
    at: 'src/odl.rs:L58-L58'
  - symbol: E_EMPTY_PROP_NAME
    kind: constant
    at: 'src/odl.rs:L59-L59'
  - symbol: E_UNKNOWN_TYPE
    kind: constant
    at: 'src/odl.rs:L60-L60'
  - symbol: E_MISSING_PROPERTY
    kind: constant
    at: 'src/odl.rs:L61-L61'
  - symbol: Section
    kind: enum
    at: 'src/odl.rs:L65-L77'
  - symbol: parse_odl
    kind: function
    at: 'src/odl.rs:L80-L361'
  - symbol: parse_header
    kind: function
    at: 'src/odl.rs:L364-L374'
  - symbol: list_item_value
    kind: function
    at: 'src/odl.rs:L378-L383'
  - symbol: parse_value_expr
    kind: function
    at: 'src/odl.rs:L386-L416'
  - symbol: flush_update
    kind: function
    at: 'src/odl.rs:L419-L423'
  - symbol: flush_create
    kind: function
    at: 'src/odl.rs:L426-L430'
  - symbol: flush_action
    kind: function
    at: 'src/odl.rs:L433-L437'
  - symbol: flush_clause
    kind: function
    at: 'src/odl.rs:L440-L444'
  - symbol: parse_compare_op
    kind: function
    at: 'src/odl.rs:L447-L456'
---
<!-- context:generated:start -->
## Summary

The language-independent intermediate representation and its deterministic parser. `ir.rs` defines the data structures (ObjectType, ActionDef, PolicyDef, ValueExpr) that everything downstream consumes; `odl.rs` is a hand-written indentation-based state machine converting .odl text into that IR, deliberately avoiding LLM-based parsing and enforcing constraints like 'every object needs ≥1 property' (error 1001). The IR is the single contract between authoring and execution.

## Related

- produces [[action-engine]] — ActionRegistry builds its type-to-action map from OntologyIR ActionDefs.
- produces [[code-generation]] — codegen consumes OntologyIR to emit Rust structs, SQL schema, and JSON.
<!-- context:generated:end -->

## Notes

_Anything written below the generated block is preserved when the graph is regenerated._
