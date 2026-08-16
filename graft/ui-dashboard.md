---
name: UI Dashboard
slug: ui-dashboard
type: system
sources:
  - path: ui/src/api.ts
    hash: 9877ae14f5d95f9c195b0c746714ec0a4efc5fa91ae8f5435a5fcf9ce14fd474
  - path: ui/src/App.tsx
    hash: e18a74c3f078e5499cfa589ad4362aec60dc2bfde351545f3689eb7d75f226d6
  - path: ui/src/hooks/useTraceStream.ts
    hash: 8ada3c0fd41d879d9402cacc3afb036b9b9aaf80aecf32ee229e64d30bf064af
  - path: ui/src/main.tsx
    hash: 5f49dce6e6cc4b5d678cf529ef892af0ab7dfa6afb300b11ad559224019b8b05
  - path: ui/src/types.ts
    hash: bfbb1589c4c32197b3f46e8b7dda8b86192ca1bd0e333b1e1f1fd198aaa3df7a
  - path: ui/src/view.ts
    hash: 0f697da5354c4991ca2e686c4611581aa9220e39162a9cad77b4ba2f9624f4e6
  - path: ui/vite.config.ts
    hash: 1eff51970c8b7a8899c843c173ed0e75079b60d0f9a04aabe61fef40f4a932cc
sources_digest: 1af69b2eb4f7eca09c96f73f43a9e9e78c5e6e4e853190eb9194984f4afc0b42
links:
  - to: server-api
    relation: uses
    description: Calls REST endpoints and streams WS events from server.rs.
  - to: timeline-views-dtos
    relation: implements
    description: ui/src/view.ts mirrors build_timeline; types.ts mirrors DTO schemas.
generator:
  version: 1
covers:
  - symbol: ViewMeta
    kind: interface
    at: 'ui/src/App.tsx:L43-L50'
  - symbol: App
    kind: function
    at: 'ui/src/App.tsx:L111-L510'
  - symbol: handleAuthError
    kind: function
    at: 'ui/src/App.tsx:L150-L159'
  - symbol: doLogin
    kind: function
    at: 'ui/src/App.tsx:L161-L170'
  - symbol: doLogout
    kind: function
    at: 'ui/src/App.tsx:L172-L182'
  - symbol: toggleGroup
    kind: function
    at: 'ui/src/App.tsx:L185-L187'
  - symbol: goView
    kind: function
    at: 'ui/src/App.tsx:L190-L196'
  - symbol: startRun
    kind: function
    at: 'ui/src/App.tsx:L221-L237'
  - symbol: AuthError
    kind: class
    at: 'ui/src/api.ts:L20-L25'
  - symbol: constructor
    kind: method
    at: 'ui/src/api.ts:L21-L24'
  - symbol: fetchWithTimeout
    kind: function
    at: 'ui/src/api.ts:L31-L44'
  - symbol: unwrap
    kind: function
    at: 'ui/src/api.ts:L46-L69'
  - symbol: authFetch
    kind: function
    at: 'ui/src/api.ts:L72-L79'
  - symbol: postLogin
    kind: function
    at: 'ui/src/api.ts:L84-L94'
  - symbol: getMe
    kind: function
    at: 'ui/src/api.ts:L97-L100'
  - symbol: postLogout
    kind: function
    at: 'ui/src/api.ts:L103-L106'
  - symbol: postRun
    kind: function
    at: 'ui/src/api.ts:L110-L117'
  - symbol: getSkillRaw
    kind: function
    at: 'ui/src/api.ts:L119-L123'
  - symbol: importSkill
    kind: function
    at: 'ui/src/api.ts:L125-L137'
  - symbol: getPlugins
    kind: function
    at: 'ui/src/api.ts:L139-L143'
  - symbol: getWorkers
    kind: function
    at: 'ui/src/api.ts:L145-L149'
  - symbol: getSessions
    kind: function
    at: 'ui/src/api.ts:L151-L155'
  - symbol: getSession
    kind: function
    at: 'ui/src/api.ts:L157-L161'
  - symbol: createSession
    kind: function
    at: 'ui/src/api.ts:L163-L169'
  - symbol: getSkills
    kind: function
    at: 'ui/src/api.ts:L173-L177'
  - symbol: getGraph
    kind: function
    at: 'ui/src/api.ts:L179-L182'
  - symbol: Options
    kind: interface
    at: 'ui/src/hooks/useTraceStream.ts:L14-L19'
  - symbol: useTraceStream
    kind: function
    at: 'ui/src/hooks/useTraceStream.ts:L21-L82'
  - symbol: connect
    kind: function
    at: 'ui/src/hooks/useTraceStream.ts:L39-L73'
  - symbol: ErrorBoundary
    kind: class
    at: 'ui/src/main.tsx:L7-L40'
  - symbol: getDerivedStateFromError
    kind: method
    at: 'ui/src/main.tsx:L13-L15'
  - symbol: render
    kind: method
    at: 'ui/src/main.tsx:L17-L39'
  - symbol: ViewKind
    kind: type
    at: 'ui/src/types.ts:L3-L14'
  - symbol: AuthUserDto
    kind: interface
    at: 'ui/src/types.ts:L17-L20'
  - symbol: TraceEventDto
    kind: interface
    at: 'ui/src/types.ts:L22-L34'
  - symbol: TimelineEvent
    kind: interface
    at: 'ui/src/types.ts:L36-L45'
  - symbol: DecisionSummary
    kind: interface
    at: 'ui/src/types.ts:L47-L51'
  - symbol: RunTimeline
    kind: interface
    at: 'ui/src/types.ts:L53-L61'
  - symbol: RunStartDto
    kind: interface
    at: 'ui/src/types.ts:L63-L67'
  - symbol: ApiResponse
    kind: interface
    at: 'ui/src/types.ts:L69-L73'
  - symbol: SkillDto
    kind: interface
    at: 'ui/src/types.ts:L77-L85'
  - symbol: PluginDto
    kind: interface
    at: 'ui/src/types.ts:L87-L94'
  - symbol: SessionEventDto
    kind: interface
    at: 'ui/src/types.ts:L96-L101'
  - symbol: WorkerDto
    kind: interface
    at: 'ui/src/types.ts:L103-L111'
  - symbol: RawSkillDto
    kind: interface
    at: 'ui/src/types.ts:L113-L119'
  - symbol: SessionSummaryDto
    kind: interface
    at: 'ui/src/types.ts:L121-L124'
  - symbol: SessionDto
    kind: interface
    at: 'ui/src/types.ts:L126-L129'
  - symbol: WorldNodeDto
    kind: interface
    at: 'ui/src/types.ts:L132-L136'
  - symbol: WorldEdgeDto
    kind: interface
    at: 'ui/src/types.ts:L138-L144'
  - symbol: WorldGraphDto
    kind: interface
    at: 'ui/src/types.ts:L146-L149'
  - symbol: toTimelineEvent
    kind: function
    at: 'ui/src/view.ts:L25-L36'
  - symbol: asString
    kind: function
    at: 'ui/src/view.ts:L38-L41'
  - symbol: deriveState
    kind: function
    at: 'ui/src/view.ts:L45-L55'
  - symbol: buildTimeline
    kind: function
    at: 'ui/src/view.ts:L57-L110'
---
<!-- context:generated:start -->
## Summary

The React observability dashboard with real-time trace streaming via WebSocket, five views (executive/operator/developer/memory/evolution), and memory/evolution panels. Enforces Learn≠Permission by keeping evolution read-only except explicit auto-approval toggles. Client-side `buildTimeline` mirrors server trimming to keep WS streams consistent with REST. Vite dev proxy forwards /api to Rust; production serves built dist from same origin.

## Related

- uses [[server-api]] — Calls REST endpoints and streams WS events from server.rs.
- implements [[timeline-views-dtos]] — ui/src/view.ts mirrors build_timeline; types.ts mirrors DTO schemas.
<!-- context:generated:end -->

## Notes

_Anything written below the generated block is preserved when the graph is regenerated._
