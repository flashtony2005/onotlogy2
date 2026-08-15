# src/ir.rs · [[ontology-ir-odl-parsing]]

- ScalarType · enum · L9-L14 — Enumerates the four supported scalar types (String, Int, Float, Bool) used across properties, params, and expressions.
- rust_type · function · L18-L25 — Maps each scalar type to its corresponding Rust type name for code generation.
- sqlite_type · function · L28-L35 — Maps each scalar type to its SQLite column type, with Bool stored as INTEGER.
- Err · type · L39-L39 — Declares the error type for ScalarType parsing as a String.
- from_str · function · L40-L48 — Parses a trimmed string into a ScalarType, rejecting unknown type names with an error.
- fmt · function · L52-L60 — Renders a ScalarType as its lowercase ODL string form for display.
- PropertyDef · struct · L65-L68 — Data holder describing a single named property with its scalar type.
- ParamDef · struct · L72-L75 — Data holder describing a single action parameter with its name and scalar type.
- ValueExpr · enum · L81-L90 — Serializable value expression enum (literal, param ref, self id, self field) interpreted by the ActionEngine to avoid per-action Rust branches.
- FieldUpdate · struct · L94-L97 — Data holder pairing a target field name with a value expression to write during an action's updates effect.
- CreateEffect · struct · L101-L113 — Describes a create effect: new object type, optional explicit id, field data expressions, and optional link edge to the target.
- CompareOp · enum · L117-L124 — Enumerates comparison operators (Lt/Le/Gt/Ge/Eq/Ne) used in policy precondition clauses.
- Clause · struct · L130-L134 — Data holder for a single policy precondition clause comparing a field or param against a value expression.
- PolicyDef · struct · L138-L145 — Holds an action's policy constraints: human-approval gate (default Deny) and AND-combined preconditions.
- ActionDef · struct · L153-L164 — Declarative action definition on an object: params, field updates, create effects, and policy constraints, executed by the generic ActionEngine.
- ObjectType · struct · L168-L174 — Data holder for an object type: its name, properties, and self-owned actions.
- OntologyIR · struct · L178-L180 — Root container holding all object types parsed from the ODL document.
- find · function · L184-L186 — Looks up an object type by name, returning a reference if present.
