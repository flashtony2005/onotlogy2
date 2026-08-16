# ui/src/App.tsx · [[learn-permission-invariant]] [[ui-dashboard]]

- ViewMeta · interface · L45-L52 — interface ViewMeta
- App · function · L121-L521 — Root component orchestrating the agent observation dashboard: manages run state, streams trace events over WebSocket, builds the timeline, and switches between Executive/Operator/Developer/Memory/Evolution views.
- handleAuthError · function · L160-L169 — handleAuthError = (e: unknown)
- doLogin · function · L171-L180 — doLogin = async (username: string, password: string)
- doLogout · function · L182-L192 — doLogout = async ()
- toggleGroup · function · L195-L197 — toggleGroup = (name: string)
- goView · function · L200-L206 — goView = (k: ViewKind)
- startRun · function · L231-L247 — Validates the goal input, triggers a new agent run via postRun, resets the event stream, and preloads existing memory before the run starts.
