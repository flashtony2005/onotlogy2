# src/lib.rs

Crate root that declares and re-exports all OpenAIP-OS kernel modules (IR, ODL, codegen, storage, runtime, agent, planner, action, trace, evolution, server) as the public API surface.

- auth · module · L16-L16 — Declares the auth module for identity providers, roles, sessions, and view clamping.
- cognitive · module · L17-L17 — pub mod cognitive;
- compose · module · L18-L18 — pub mod compose;
- diagnosis · module · L19-L19 — pub mod diagnosis;
- fabric · module · L20-L20 — pub mod fabric;
- facade · module · L21-L21 — pub mod facade;
- evaluation · module · L22-L25 — pub mod evaluation
- hook · module · L24-L24 — pub mod hook;
- evolution2 · module · L26-L26 — pub mod evolution2;
- health · module · L27-L27 — pub mod health;
- mutation · module · L37-L41 — pub mod mutation
- benchmark · module · L39-L39 — pub mod benchmark;
- plugin · module · L42-L42 — pub mod plugin;
- projection · module · L43-L43 — pub mod projection;
- rule · module · L44-L44 — pub mod rule;
- server · module · L45-L45 — Declares the server module hosting axum REST, WebSocket streaming, and three-layer observation UI.
- simulation · module · L46-L46 — pub mod simulation;
- skill · module · L47-L47 — pub mod skill;
- worker · module · L48-L48 — pub mod worker;
- world · module · L49-L49 — pub mod world;
- world_exec · module · L50-L50 — pub mod world_exec;
