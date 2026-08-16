---
name: Learn≠Permission Invariant
slug: learn-permission-invariant
type: concept
sources:
  - path: src/evolution.rs
    hash: 8b22e1db63cf8cb31cf6cdc3f9664b067373f58a2c20046e9cfc21657c44cef0
  - path: src/planner.rs
    hash: 6731138539e062f8a3b9ca2b639536609861c30ea54fac8d5ffed8ea0d5f9a13
  - path: src/runtime.rs
    hash: 5f2cab0b89c4304f864d96ccf1b2be11dc741f2d472d3eae69974bfe62cec174
  - path: ui/src/api.ts
    hash: 9877ae14f5d95f9c195b0c746714ec0a4efc5fa91ae8f5435a5fcf9ce14fd474
  - path: ui/src/App.tsx
    hash: e18a74c3f078e5499cfa589ad4362aec60dc2bfde351545f3689eb7d75f226d6
sources_digest: b6b660a91644fa18eeef20d1cfe218708561a1b54b7ff3e7b14b59f55b1c29d5
links:
  - to: evolution-engine
    relation: implements
    description: The pipeline enforces the separation of knowledge from permissions.
  - to: planner-policy-engine
    relation: implements
    description: may_auto_approve gates and audit-before-execute enforce the invariant.
generator:
  version: 1
covers:
  - symbol: ViewMeta
    kind: interface
    at: 'ui/src/App.tsx:L45-L52'
  - symbol: App
    kind: function
    at: 'ui/src/App.tsx:L121-L521'
  - symbol: handleAuthError
    kind: function
    at: 'ui/src/App.tsx:L160-L169'
  - symbol: doLogin
    kind: function
    at: 'ui/src/App.tsx:L171-L180'
  - symbol: doLogout
    kind: function
    at: 'ui/src/App.tsx:L182-L192'
  - symbol: toggleGroup
    kind: function
    at: 'ui/src/App.tsx:L195-L197'
  - symbol: goView
    kind: function
    at: 'ui/src/App.tsx:L200-L206'
  - symbol: startRun
    kind: function
    at: 'ui/src/App.tsx:L231-L247'
  - symbol: AuthError
    kind: class
    at: 'ui/src/api.ts:L26-L31'
  - symbol: constructor
    kind: method
    at: 'ui/src/api.ts:L27-L30'
  - symbol: fetchWithTimeout
    kind: function
    at: 'ui/src/api.ts:L37-L50'
  - symbol: unwrap
    kind: function
    at: 'ui/src/api.ts:L52-L75'
  - symbol: authFetch
    kind: function
    at: 'ui/src/api.ts:L78-L85'
  - symbol: postLogin
    kind: function
    at: 'ui/src/api.ts:L90-L100'
  - symbol: getMe
    kind: function
    at: 'ui/src/api.ts:L103-L106'
  - symbol: postLogout
    kind: function
    at: 'ui/src/api.ts:L109-L112'
  - symbol: postRun
    kind: function
    at: 'ui/src/api.ts:L116-L123'
  - symbol: getSkillRaw
    kind: function
    at: 'ui/src/api.ts:L125-L129'
  - symbol: importSkill
    kind: function
    at: 'ui/src/api.ts:L131-L143'
  - symbol: getPlugins
    kind: function
    at: 'ui/src/api.ts:L145-L149'
  - symbol: getWorkers
    kind: function
    at: 'ui/src/api.ts:L151-L155'
  - symbol: getSessions
    kind: function
    at: 'ui/src/api.ts:L157-L161'
  - symbol: getSession
    kind: function
    at: 'ui/src/api.ts:L163-L167'
  - symbol: createSession
    kind: function
    at: 'ui/src/api.ts:L169-L175'
  - symbol: getSkills
    kind: function
    at: 'ui/src/api.ts:L179-L183'
  - symbol: getGraph
    kind: function
    at: 'ui/src/api.ts:L185-L188'
  - symbol: postWorldQuery
    kind: function
    at: 'ui/src/api.ts:L194-L205'
  - symbol: postAuditQuery
    kind: function
    at: 'ui/src/api.ts:L207-L218'
  - symbol: postFabricSubmit
    kind: function
    at: 'ui/src/api.ts:L220-L231'
---
<!-- context:generated:start -->
## Summary

A cross-cutting design invariant: the evolution engine may convert experiences into validated knowledge but must never modify policies or bypass human approval. Enforced at multiple layers — evolution.rs separates knowledge validation from permission changes, the UI keeps evolution read-only except explicit auto-approval toggles, and auto-approval always writes an audit record before execution. The revocable auto_approve_enabled flag defaults to true but can force human approval.

## Related

- implements [[evolution-engine]] — The pipeline enforces the separation of knowledge from permissions.
- implements [[planner-policy-engine]] — may_auto_approve gates and audit-before-execute enforce the invariant.
<!-- context:generated:end -->

## Notes

_Anything written below the generated block is preserved when the graph is regenerated._
