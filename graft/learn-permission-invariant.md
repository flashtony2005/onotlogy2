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
    at: 'ui/src/App.tsx:L47-L54'
  - symbol: App
    kind: function
    at: 'ui/src/App.tsx:L131-L532'
  - symbol: handleAuthError
    kind: function
    at: 'ui/src/App.tsx:L170-L179'
  - symbol: doLogin
    kind: function
    at: 'ui/src/App.tsx:L181-L190'
  - symbol: doLogout
    kind: function
    at: 'ui/src/App.tsx:L192-L202'
  - symbol: toggleGroup
    kind: function
    at: 'ui/src/App.tsx:L205-L207'
  - symbol: goView
    kind: function
    at: 'ui/src/App.tsx:L210-L216'
  - symbol: startRun
    kind: function
    at: 'ui/src/App.tsx:L241-L257'
  - symbol: AuthError
    kind: class
    at: 'ui/src/api.ts:L28-L33'
  - symbol: constructor
    kind: method
    at: 'ui/src/api.ts:L29-L32'
  - symbol: fetchWithTimeout
    kind: function
    at: 'ui/src/api.ts:L39-L52'
  - symbol: unwrap
    kind: function
    at: 'ui/src/api.ts:L54-L77'
  - symbol: authFetch
    kind: function
    at: 'ui/src/api.ts:L80-L87'
  - symbol: postLogin
    kind: function
    at: 'ui/src/api.ts:L92-L102'
  - symbol: getMe
    kind: function
    at: 'ui/src/api.ts:L105-L108'
  - symbol: postLogout
    kind: function
    at: 'ui/src/api.ts:L111-L114'
  - symbol: postRun
    kind: function
    at: 'ui/src/api.ts:L118-L125'
  - symbol: getSkillRaw
    kind: function
    at: 'ui/src/api.ts:L127-L131'
  - symbol: importSkill
    kind: function
    at: 'ui/src/api.ts:L133-L145'
  - symbol: getPlugins
    kind: function
    at: 'ui/src/api.ts:L147-L151'
  - symbol: getWorkers
    kind: function
    at: 'ui/src/api.ts:L153-L157'
  - symbol: getSessions
    kind: function
    at: 'ui/src/api.ts:L159-L163'
  - symbol: getSession
    kind: function
    at: 'ui/src/api.ts:L165-L169'
  - symbol: createSession
    kind: function
    at: 'ui/src/api.ts:L171-L177'
  - symbol: getSkills
    kind: function
    at: 'ui/src/api.ts:L181-L185'
  - symbol: getGraph
    kind: function
    at: 'ui/src/api.ts:L187-L190'
  - symbol: postWorldQuery
    kind: function
    at: 'ui/src/api.ts:L196-L207'
  - symbol: postAuditQuery
    kind: function
    at: 'ui/src/api.ts:L209-L220'
  - symbol: postProjectList
    kind: function
    at: 'ui/src/api.ts:L222-L231'
  - symbol: postProjectSeed
    kind: function
    at: 'ui/src/api.ts:L233-L242'
  - symbol: postProjectCreate
    kind: function
    at: 'ui/src/api.ts:L244-L253'
  - symbol: postFabricSubmit
    kind: function
    at: 'ui/src/api.ts:L255-L266'
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
