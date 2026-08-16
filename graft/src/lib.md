# src/lib.rs

Crate root that declares and re-exports all OpenAIP-OS kernel modules (IR, ODL, codegen, storage, runtime, agent, planner, action, trace, evolution, server) as the public API surface.

- auth · module · L16-L16 — Declares the auth module for identity providers, roles, sessions, and view clamping.
- cognitive · module · L17-L17 — pub mod cognitive;
- compose · module · L18-L18 — pub mod compose;
- diagnosis · module · L19-L19 — pub mod diagnosis;
- fabric · module · L20-L20 — pub mod fabric;
- evaluation · module · L21-L24 — pub mod evaluation
- hook · module · L23-L23 — pub mod hook;
- evolution2 · module · L25-L25 — pub mod evolution2;
- health · module · L26-L26 — pub mod health;
- mutation · module · L36-L40 — pub mod mutation
- benchmark · module · L38-L38 — pub mod benchmark;
- plugin · module · L41-L41 — pub mod plugin;
- projection · module · L42-L42 — pub mod projection;
- rule · module · L43-L43 — pub mod rule;
- server · module · L44-L44 — Declares the server module hosting axum REST, WebSocket streaming, and three-layer observation UI.
- simulation · module · L45-L45 — pub mod simulation;
- skill · module · L46-L46 — pub mod skill;
- worker · module · L47-L47 — pub mod worker;
- world · module · L48-L48 — pub mod world;
- world_exec · module · L49-L49 — pub mod world_exec;
