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
    at: 'ui/src/types.ts:L3-L15'
  - symbol: AuthUserDto
    kind: interface
    at: 'ui/src/types.ts:L18-L21'
  - symbol: TraceEventDto
    kind: interface
    at: 'ui/src/types.ts:L23-L35'
  - symbol: TimelineEvent
    kind: interface
    at: 'ui/src/types.ts:L37-L46'
  - symbol: DecisionSummary
    kind: interface
    at: 'ui/src/types.ts:L48-L52'
  - symbol: RunTimeline
    kind: interface
    at: 'ui/src/types.ts:L54-L62'
  - symbol: RunStartDto
    kind: interface
    at: 'ui/src/types.ts:L64-L68'
  - symbol: ApiResponse
    kind: interface
    at: 'ui/src/types.ts:L70-L74'
  - symbol: SkillDto
    kind: interface
    at: 'ui/src/types.ts:L78-L86'
  - symbol: PluginDto
    kind: interface
    at: 'ui/src/types.ts:L88-L95'
  - symbol: SessionEventDto
    kind: interface
    at: 'ui/src/types.ts:L97-L102'
  - symbol: WorkerDto
    kind: interface
    at: 'ui/src/types.ts:L104-L112'
  - symbol: RawSkillDto
    kind: interface
    at: 'ui/src/types.ts:L114-L120'
  - symbol: SessionSummaryDto
    kind: interface
    at: 'ui/src/types.ts:L122-L125'
  - symbol: SessionDto
    kind: interface
    at: 'ui/src/types.ts:L127-L130'
  - symbol: WorldNodeDto
    kind: interface
    at: 'ui/src/types.ts:L133-L137'
  - symbol: WorldEdgeDto
    kind: interface
    at: 'ui/src/types.ts:L139-L145'
  - symbol: WorldGraphDto
    kind: interface
    at: 'ui/src/types.ts:L147-L150'
  - symbol: WorldRelationDto
    kind: interface
    at: 'ui/src/types.ts:L155-L161'
  - symbol: WorldViewDto
    kind: interface
    at: 'ui/src/types.ts:L163-L167'
  - symbol: WorldQueryDto
    kind: interface
    at: 'ui/src/types.ts:L169-L173'
  - symbol: AuditEventDto
    kind: interface
    at: 'ui/src/types.ts:L175-L181'
  - symbol: AuditViewDto
    kind: interface
    at: 'ui/src/types.ts:L183-L186'
  - symbol: AuditQueryDto
    kind: interface
    at: 'ui/src/types.ts:L188-L193'
  - symbol: FabricRequestDto
    kind: interface
    at: 'ui/src/types.ts:L195-L200'
  - symbol: FabricSubmissionDto
    kind: interface
    at: 'ui/src/types.ts:L202-L217'
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
