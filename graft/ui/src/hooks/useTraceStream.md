# ui/src/hooks/useTraceStream.ts · [[ui-dashboard]]

WebSocket hook that streams live agent trace events with exponential-backoff reconnection and history catch-up via the after parameter.

- Options · interface · L14-L19 — Callback contract for the trace stream: delivers each parsed event and optional connection-status changes.
- useTraceStream · function · L21-L82 — Subscribes to the agent trace WebSocket for a given id, resetting history on new runs and tearing down the socket on unmount or id change.
- connect · function · L39-L73 — Opens the WebSocket, appending the last event id as a query param to resume history, and schedules reconnects with capped exponential backoff on close.
