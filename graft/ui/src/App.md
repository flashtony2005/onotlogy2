# ui/src/App.tsx · [[learn-permission-invariant]] [[ui-dashboard]]

- ViewMeta · interface · L43-L50 — interface ViewMeta
- App · function · L111-L510 — Root component orchestrating the agent observation dashboard: manages run state, streams trace events over WebSocket, builds the timeline, and switches between Executive/Operator/Developer/Memory/Evolution views.
- handleAuthError · function · L150-L159 — handleAuthError = (e: unknown)
- doLogin · function · L161-L170 — doLogin = async (username: string, password: string)
- doLogout · function · L172-L182 — doLogout = async ()
- toggleGroup · function · L185-L187 — toggleGroup = (name: string)
- goView · function · L190-L196 — goView = (k: ViewKind)
- startRun · function · L221-L237 — Validates the goal input, triggers a new agent run via postRun, resets the event stream, and preloads existing memory before the run starts.
