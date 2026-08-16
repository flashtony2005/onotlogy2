# src/lib.rs

Crate root that declares and re-exports all OpenAIP-OS kernel modules (IR, ODL, codegen, storage, runtime, agent, planner, action, trace, evolution, server) as the public API surface.

- auth · module · L16-L16 — Declares the auth module for identity providers, roles, sessions, and view clamping.
- codegen · module · L17-L17 — Declares the codegen module that generates Rust structs, schema.sql, and ontology.json from IR.
- cognitive · module · L18-L18 — pub mod cognitive;
- compose · module · L19-L19 — pub mod compose;
- diagnosis · module · L20-L20 — pub mod diagnosis;
- evaluation · module · L21-L21 — pub mod evaluation;
- evolution2 · module · L22-L22 — pub mod evolution2;
- health · module · L23-L23 — pub mod health;
- ir · module · L24-L24 — Declares the ir module defining the language-independent Ontology IR (ObjectType/PropertyDef/ScalarType).
- kernel · module · L25-L25 — pub mod kernel;
- mutation · module · L26-L26 — pub mod mutation;
- odl · module · L27-L27 — Declares the odl module that deterministically parses ODL text into IR without LLM involvement.
- plugin · module · L28-L28 — pub mod plugin;
- rule · module · L29-L29 — pub mod rule;
- server · module · L30-L30 — Declares the server module hosting axum REST, WebSocket streaming, and three-layer observation UI.
- session · module · L31-L31 — pub mod session;
- simulation · module · L32-L32 — pub mod simulation;
- skill · module · L33-L33 — pub mod skill;
- storage · module · L34-L34 — Declares the storage module with ObjectEngine/RelationEngine abstractions and LibSQLStorage (Turso) backend.
- trace · module · L35-L35 — Declares the trace module providing the three-layer observation timeline and TraceEventDto.
- worker · module · L36-L36 — pub mod worker;
- world · module · L37-L37 — pub mod world;
- world_exec · module · L38-L38 — pub mod world_exec;
