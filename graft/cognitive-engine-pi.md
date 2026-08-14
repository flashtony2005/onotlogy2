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
covers:
  - symbol: AgentError
    kind: enum
    at: 'src/agent.rs:L18-L25'
  - symbol: WorldSnapshot
    kind: struct
    at: 'src/agent.rs:L29-L34'
  - symbol: Context
    kind: struct
    at: 'src/agent.rs:L38-L51'
  - symbol: Decision
    kind: struct
    at: 'src/agent.rs:L55-L66'
  - symbol: AgentEngine
    kind: interface
    at: 'src/agent.rs:L70-L80'
  - symbol: name
    kind: function
    at: 'src/agent.rs:L72-L72'
  - symbol: reason
    kind: function
    at: 'src/agent.rs:L74-L74'
  - symbol: health
    kind: function
    at: 'src/agent.rs:L77-L79'
  - symbol: PiAgentEngine
    kind: struct
    at: 'src/agent.rs:L88-L94'
  - symbol: new
    kind: function
    at: 'src/agent.rs:L100-L125'
  - symbol: with_provider
    kind: function
    at: 'src/agent.rs:L128-L131'
  - symbol: with_model
    kind: function
    at: 'src/agent.rs:L134-L137'
  - symbol: with_api_key_env
    kind: function
    at: 'src/agent.rs:L140-L143'
  - symbol: system_prompt
    kind: function
    at: 'src/agent.rs:L146-L155'
  - symbol: user_prompt
    kind: function
    at: 'src/agent.rs:L158-L168'
  - symbol: parse_decision
    kind: function
    at: 'src/agent.rs:L171-L212'
  - symbol: name
    kind: function
    at: 'src/agent.rs:L217-L219'
  - symbol: health
    kind: function
    at: 'src/agent.rs:L221-L223'
  - symbol: reason
    kind: function
    at: 'src/agent.rs:L225-L283'
  - symbol: which_pi
    kind: function
    at: 'src/agent.rs:L287-L305'
  - symbol: extract_text
    kind: function
    at: 'src/agent.rs:L308-L313'
  - symbol: strip_code_fence
    kind: function
    at: 'src/agent.rs:L316-L325'
  - symbol: extract_text_from_value
    kind: function
    at: 'src/agent.rs:L328-L350'
  - symbol: collect_refs
    kind: function
    at: 'src/agent.rs:L353-L359'
---
<!-- context:generated:start -->
## Summary

The pluggable LLM backend layer. `AgentEngine` trait abstracts the cognitive engine; `PiAgentEngine` shells out to the `pi` CLI in non-interactive text mode with `--no-tools` to prevent filesystem access, parsing JSON output with best-effort fallback. Health-checks via `which_pi()` to fail fast, 120s timeout, and configurable provider/model/API key via env vars. Strictly a cognitive engine — the Planner owns all decision-making.

## Related

- implements [[planner-policy-engine]] — Implements the AgentEngine trait the Planner calls for Decisions.
<!-- context:generated:end -->

## Notes

_Anything written below the generated block is preserved when the graph is regenerated._
