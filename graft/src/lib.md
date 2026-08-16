# src/lib.rs

Crate root that declares and re-exports all OpenAIP-OS kernel modules (IR, ODL, codegen, storage, runtime, agent, planner, action, trace, evolution, server) as the public API surface.

- auth · module · L16-L16 — Declares the auth module for identity providers, roles, sessions, and view clamping.
- cognitive · module · L17-L17 — pub mod cognitive;
- compose · module · L18-L18 — pub mod compose;
- diagnosis · module · L19-L19 — pub mod diagnosis;
- evaluation · module · L20-L20 — pub mod evaluation;
- evolution2 · module · L21-L21 — pub mod evolution2;
- health · module · L22-L22 — pub mod health;
- mutation · module · L25-L25 — pub mod mutation;
- plugin · module · L26-L26 — pub mod plugin;
- rule · module · L27-L27 — pub mod rule;
- server · module · L28-L28 — Declares the server module hosting axum REST, WebSocket streaming, and three-layer observation UI.
- session · module · L29-L29 — pub mod session;
- simulation · module · L30-L30 — pub mod simulation;
- skill · module · L31-L31 — pub mod skill;
- storage · module · L32-L32 — Declares the storage module with ObjectEngine/RelationEngine abstractions and LibSQLStorage (Turso) backend.
- trace · module · L33-L33 — Declares the trace module providing the three-layer observation timeline and TraceEventDto.
- worker · module · L34-L34 — pub mod worker;
- world · module · L35-L35 — pub mod world;
- world_exec · module · L36-L36 — pub mod world_exec;
