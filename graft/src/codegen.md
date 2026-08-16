# src/codegen.rs · [[code-generation]]

- CodegenError · struct · L12-L15 — Typed error carrying a numeric code and message for codegen failures.
- new · function · L18-L23 — Constructor building a CodegenError from a code and message.
- CodegenOutput · struct · L28-L35 — Aggregate of the three codegen artifacts: Rust structs, schema DDL, and ontology JSON.
- ObjectDef · struct · L39-L42 — Compile-time object type definition holding serialized JSON so runtime behavior is serde-driven instead of hardcoded match branches.
- codegen · function · L45-L59 — Top-level entry generating all three artifacts from an IR, joining per-type structs and serializing the IR to pretty JSON.
- gen_struct · function · L62-L81 — Emits a Rust struct for one object type plus a compile-time <TYPE>_DEF constant embedding the type's serialized JSON for runtime serde-driven use.
- canonical_schema · function · L84-L267 — Returns the canonical normalized core-table DDL (unified object/relation/trace/memory plus auth, run, and evolution tables), enforcing the no-per-type-table constraint.
- to_json · function · L270-L272 — Serializes the IR to pretty JSON for writing ontology.json.
- roundtrip_ok · function · L275-L280 — Verifies serialization round-trip fidelity (TC-002) by deserializing the JSON back and comparing equality with the original IR.
- _assert_serialize · function · L284-L284 — Compile-time assertion that Serialize is in scope for tests, avoiding unused-import warnings.
