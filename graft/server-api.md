---
name: Server & API
slug: server-api
type: system
sources:
  - path: src/server.rs
    hash: 453cc3dbe5459733726e6d4412099744eda5e67d77abf9f5b946c84a670b5c4a
sources_digest: 42cb53b408eb7592b30a2791f305902bd86ce0c046ad4b88e85f1fd1ab74f6e0
links:
  - to: auth-rbac
    relation: uses
    description: Auth middleware enforces roles and injects AuthUser.
  - to: planner-policy-engine
    relation: uses
    description: ServerApprover handles HTTP-based approval; Planner drives runs.
  - to: runtime-kernel-observability
    relation: uses
    description: Integrates RuntimeKernel for run state and event streaming.
generator:
  version: 1
covers: []
---
<!-- context:generated:start -->
## Summary

The axum HTTP/WebSocket server exposing REST endpoints for traces, timelines, memory, evolution, and approvals, plus WS streaming with `?after` reconnection. Enforces auth via middleware, uses a unified ApiResponse envelope with WB error codes, and rebuilds in-memory run state from persisted storage on restart. Serves static UI from ui/dist. Notable gotchas: dual approval paths (legacy HTTP vs evolution auto-approve) and camelCase DTOs as single source of truth for both WS and timeline.

## Related

- uses [[auth-rbac]] — Auth middleware enforces roles and injects AuthUser.
- uses [[planner-policy-engine]] — ServerApprover handles HTTP-based approval; Planner drives runs.
- uses [[runtime-kernel-observability]] — Integrates RuntimeKernel for run state and event streaming.
<!-- context:generated:end -->

## Notes

_Anything written below the generated block is preserved when the graph is regenerated._
