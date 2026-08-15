# src/lib.rs

Crate root that declares and re-exports all OpenAIP-OS kernel modules (IR, ODL, codegen, storage, runtime, agent, planner, action, trace, evolution, server) as the public API surface.

- action · module · L16-L16 — Declares the action module (V0.4 Action & Policy engine) for interpreting and executing object-owned actions.
- agent · module · L17-L17 — Declares the agent module providing the AgentEngine trait and PiAgentEngine implementation.
- auth · module · L18-L18 — Declares the auth module for identity providers, roles, sessions, and view clamping.
- codegen · module · L19-L19 — Declares the codegen module that generates Rust structs, schema.sql, and ontology.json from IR.
- evolution · module · L20-L20 — Declares the evolution module (BL-003/REQ-017) for observable, verifiable, revocable learned-knowledge extension on top of the kernel.
- ir · module · L21-L21 — Declares the ir module defining the language-independent Ontology IR (ObjectType/PropertyDef/ScalarType).
- odl · module · L22-L22 — Declares the odl module that deterministically parses ODL text into IR without LLM involvement.
- planner · module · L23-L23 — Declares the planner module implementing the OpenAIP goal-to-plan-to-capability execution pipeline.
- runtime · module · L24-L24 — Declares the runtime module providing TraceEvent and EventBus as the explainability foundation.
- server · module · L25-L25 — Declares the server module hosting axum REST, WebSocket streaming, and three-layer observation UI.
- storage · module · L26-L26 — Declares the storage module with ObjectEngine/RelationEngine abstractions and LibSQLStorage (Turso) backend.
- trace · module · L27-L27 — Declares the trace module providing the three-layer observation timeline and TraceEventDto.
