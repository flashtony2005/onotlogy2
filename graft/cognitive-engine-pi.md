---
name: Cognitive Engine (Pi)
slug: cognitive-engine-pi
type: system
sources:
  - path: src/agent.rs
    hash: b11ed6e71c84a66c007d69adb3abc8278dc33af338fd8e3b60be2a381fa6553a
sources_digest: 3ccd74e9bba366a6f522e521733e9cedef78ceb3bf657312a5bcf5a6a44ac5a7
links:
  - to: planner-policy-engine
    relation: implements
    description: Implements the AgentEngine trait the Planner calls for Decisions.
generator:
  version: 1
covers: []
---
<!-- context:generated:start -->
## Summary

The pluggable LLM backend layer. `AgentEngine` trait abstracts the cognitive engine; `PiAgentEngine` shells out to the `pi` CLI in non-interactive text mode with `--no-tools` to prevent filesystem access, parsing JSON output with best-effort fallback. Health-checks via `which_pi()` to fail fast, 120s timeout, and configurable provider/model/API key via env vars. Strictly a cognitive engine — the Planner owns all decision-making.

## Related

- implements [[planner-policy-engine]] — Implements the AgentEngine trait the Planner calls for Decisions.
<!-- context:generated:end -->

## Notes

_Anything written below the generated block is preserved when the graph is regenerated._
