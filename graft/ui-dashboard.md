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
    at: 'ui/src/types.ts:L3-L16'
  - symbol: AuthUserDto
    kind: interface
    at: 'ui/src/types.ts:L19-L22'
  - symbol: TraceEventDto
    kind: interface
    at: 'ui/src/types.ts:L24-L36'
  - symbol: TimelineEvent
    kind: interface
    at: 'ui/src/types.ts:L38-L47'
  - symbol: DecisionSummary
    kind: interface
    at: 'ui/src/types.ts:L49-L53'
  - symbol: RunTimeline
    kind: interface
    at: 'ui/src/types.ts:L55-L63'
  - symbol: RunStartDto
    kind: interface
    at: 'ui/src/types.ts:L65-L69'
  - symbol: ApiResponse
    kind: interface
    at: 'ui/src/types.ts:L71-L75'
  - symbol: SkillDto
    kind: interface
    at: 'ui/src/types.ts:L79-L87'
  - symbol: PluginDto
    kind: interface
    at: 'ui/src/types.ts:L89-L96'
  - symbol: SessionEventDto
    kind: interface
    at: 'ui/src/types.ts:L98-L103'
  - symbol: WorkerDto
    kind: interface
    at: 'ui/src/types.ts:L105-L113'
  - symbol: RawSkillDto
    kind: interface
    at: 'ui/src/types.ts:L115-L121'
  - symbol: SessionSummaryDto
    kind: interface
    at: 'ui/src/types.ts:L123-L126'
  - symbol: SessionDto
    kind: interface
    at: 'ui/src/types.ts:L128-L131'
  - symbol: WorldNodeDto
    kind: interface
    at: 'ui/src/types.ts:L134-L138'
  - symbol: WorldEdgeDto
    kind: interface
    at: 'ui/src/types.ts:L140-L146'
  - symbol: WorldGraphDto
    kind: interface
    at: 'ui/src/types.ts:L148-L151'
  - symbol: WorldRelationDto
    kind: interface
    at: 'ui/src/types.ts:L156-L162'
  - symbol: WorldViewDto
    kind: interface
    at: 'ui/src/types.ts:L164-L168'
  - symbol: WorldQueryDto
    kind: interface
    at: 'ui/src/types.ts:L170-L175'
  - symbol: AuditEventDto
    kind: interface
    at: 'ui/src/types.ts:L177-L183'
  - symbol: AuditViewDto
    kind: interface
    at: 'ui/src/types.ts:L185-L188'
  - symbol: AuditQueryDto
    kind: interface
    at: 'ui/src/types.ts:L190-L196'
  - symbol: FabricRequestDto
    kind: interface
    at: 'ui/src/types.ts:L198-L204'
  - symbol: FabricSubmissionDto
    kind: interface
    at: 'ui/src/types.ts:L206-L221'
  - symbol: ProjectDto
    kind: interface
    at: 'ui/src/types.ts:L225-L232'
  - symbol: ProjectCreateDto
    kind: interface
    at: 'ui/src/types.ts:L234-L239'
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
