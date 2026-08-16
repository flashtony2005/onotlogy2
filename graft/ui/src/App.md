# ui/src/App.tsx · [[learn-permission-invariant]] [[ui-dashboard]]

- ViewMeta · interface · L47-L54 — interface ViewMeta
- App · function · L131-L532 — Root component orchestrating the agent observation dashboard: manages run state, streams trace events over WebSocket, builds the timeline, and switches between Executive/Operator/Developer/Memory/Evolution views.
- handleAuthError · function · L170-L179 — handleAuthError = (e: unknown)
- doLogin · function · L181-L190 — doLogin = async (username: string, password: string)
- doLogout · function · L192-L202 — doLogout = async ()
- toggleGroup · function · L205-L207 — toggleGroup = (name: string)
- goView · function · L210-L216 — goView = (k: ViewKind)
- startRun · function · L241-L257 — Validates the goal input, triggers a new agent run via postRun, resets the event stream, and preloads existing memory before the run starts.
