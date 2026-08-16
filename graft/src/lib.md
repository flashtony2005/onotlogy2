# src/lib.rs

Crate root that declares and re-exports all OpenAIP-OS kernel modules (IR, ODL, codegen, storage, runtime, agent, planner, action, trace, evolution, server) as the public API surface.

- auth · module · L16-L16 — Declares the auth module for identity providers, roles, sessions, and view clamping.
- cognitive · module · L17-L17 — pub mod cognitive;
- compose · module · L18-L18 — pub mod compose;
- diagnosis · module · L19-L19 — pub mod diagnosis;
- evaluation · module · L20-L23 — pub mod evaluation
- hook · module · L22-L22 — pub mod hook;
- evolution2 · module · L24-L24 — pub mod evolution2;
- health · module · L25-L25 — pub mod health;
- mutation · module · L34-L38 — pub mod mutation
- benchmark · module · L36-L36 — pub mod benchmark;
- plugin · module · L39-L39 — pub mod plugin;
- rule · module · L40-L40 — pub mod rule;
- server · module · L41-L41 — Declares the server module hosting axum REST, WebSocket streaming, and three-layer observation UI.
- simulation · module · L42-L42 — pub mod simulation;
- skill · module · L43-L43 — pub mod skill;
- worker · module · L44-L44 — pub mod worker;
- world · module · L45-L45 — pub mod world;
- world_exec · module · L46-L46 — pub mod world_exec;
