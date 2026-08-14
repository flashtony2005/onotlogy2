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
  - symbol: fmtTime
    kind: function
    at: 'ui/src/App.tsx:L33-L37'
  - symbol: fmtJson
    kind: function
    at: 'ui/src/App.tsx:L39-L47'
  - symbol: App
    kind: function
    at: 'ui/src/App.tsx:L49-L273'
  - symbol: reloadMemory
    kind: function
    at: 'ui/src/App.tsx:L62-L72'
  - symbol: reloadEvolution
    kind: function
    at: 'ui/src/App.tsx:L78-L88'
  - symbol: startRun
    kind: function
    at: 'ui/src/App.tsx:L106-L123'
  - symbol: MemoryPanel
    kind: function
    at: 'ui/src/App.tsx:L276-L316'
  - symbol: EvolutionPanel
    kind: function
    at: 'ui/src/App.tsx:L319-L466'
  - symbol: toggleAutoApprove
    kind: function
    at: 'ui/src/App.tsx:L337-L347'
  - symbol: showAudit
    kind: function
    at: 'ui/src/App.tsx:L349-L352'
  - symbol: unwrap
    kind: function
    at: 'ui/src/api.ts:L13-L19'
  - symbol: postRun
    kind: function
    at: 'ui/src/api.ts:L21-L28'
  - symbol: getTimeline
    kind: function
    at: 'ui/src/api.ts:L30-L35'
  - symbol: getMemory
    kind: function
    at: 'ui/src/api.ts:L38-L41'
  - symbol: getEvolution
    kind: function
    at: 'ui/src/api.ts:L45-L48'
  - symbol: postAutoApproveToggle
    kind: function
    at: 'ui/src/api.ts:L52-L62'
  - symbol: getApprovalAudit
    kind: function
    at: 'ui/src/api.ts:L65-L68'
  - symbol: Options
    kind: interface
    at: 'ui/src/hooks/useTraceStream.ts:L8-L11'
  - symbol: useTraceStream
    kind: function
    at: 'ui/src/hooks/useTraceStream.ts:L13-L63'
  - symbol: connect
    kind: function
    at: 'ui/src/hooks/useTraceStream.ts:L28-L54'
  - symbol: ViewKind
    kind: type
    at: 'ui/src/types.ts:L3-L3'
  - symbol: TraceEventDto
    kind: interface
    at: 'ui/src/types.ts:L5-L17'
  - symbol: TimelineEvent
    kind: interface
    at: 'ui/src/types.ts:L19-L28'
  - symbol: DecisionSummary
    kind: interface
    at: 'ui/src/types.ts:L30-L34'
  - symbol: RunTimeline
    kind: interface
    at: 'ui/src/types.ts:L36-L44'
  - symbol: RunStartDto
    kind: interface
    at: 'ui/src/types.ts:L46-L50'
  - symbol: ApiResponse
    kind: interface
    at: 'ui/src/types.ts:L52-L56'
  - symbol: MemoryRecord
    kind: interface
    at: 'ui/src/types.ts:L59-L65'
  - symbol: MemoryListDto
    kind: interface
    at: 'ui/src/types.ts:L67-L70'
  - symbol: PatternDto
    kind: interface
    at: 'ui/src/types.ts:L74-L80'
  - symbol: ProposalDto
    kind: interface
    at: 'ui/src/types.ts:L82-L90'
  - symbol: LearnedDto
    kind: interface
    at: 'ui/src/types.ts:L92-L102'
  - symbol: ApprovalAuditDto
    kind: interface
    at: 'ui/src/types.ts:L105-L113'
  - symbol: EvolutionStateDto
    kind: interface
    at: 'ui/src/types.ts:L115-L119'
  - symbol: toTimelineEvent
    kind: function
    at: 'ui/src/view.ts:L24-L35'
  - symbol: asString
    kind: function
    at: 'ui/src/view.ts:L37-L40'
  - symbol: buildTimeline
    kind: function
    at: 'ui/src/view.ts:L42-L97'
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
