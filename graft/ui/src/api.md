# ui/src/api.ts · [[learn-permission-invariant]] [[ui-dashboard]]

REST client module for the agent backend, wrapping all API calls in the {code,msg,data} envelope with /api/v1/wb/agent prefix.

- AuthError · class · L20-L25 — class AuthError extends Error
- constructor · method · L21-L24 — constructor(msg = '登录已过期，请重新登录')
- fetchWithTimeout · function · L31-L44 — async function fetchWithTimeout(input: string, init?: RequestInit): Promise<Response>
- unwrap · function · L46-L69 — Unwraps the backend envelope, throwing on non-zero code or missing data so callers get typed payloads or a clear error.
- authFetch · function · L72-L79 — async function authFetch(input: string, init?: RequestInit): Promise<Response>
- postLogin · function · L84-L94 — async function postLogin( username: string, password: string, ): Promise<{ token: string; user: AuthUserDto }>
- getMe · function · L97-L100 — async function getMe(): Promise<AuthUserDto>
- postLogout · function · L103-L106 — async function postLogout(): Promise<void>
- postRun · function · L110-L117 — Starts a new agent run by POSTing the agent id and goal to the run endpoint.
- getSkillRaw · function · L119-L123 — async function getSkillRaw(skillId: string): Promise<RawSkillDto>
- importSkill · function · L125-L137 — async function importSkill( name: string, content: string, knownCapabilities: string[], ): Promise<SkillDto>
- getPlugins · function · L139-L143 — async function getPlugins(): Promise<PluginDto[]>
- getWorkers · function · L145-L149 — async function getWorkers(): Promise<WorkerDto[]>
- getSessions · function · L151-L155 — async function getSessions(): Promise<SessionSummaryDto[]>
- getSession · function · L157-L161 — async function getSession(sessionId: string): Promise<SessionDto>
- createSession · function · L163-L169 — async function createSession(sessionId: string): Promise<SessionDto>
- getSkills · function · L173-L177 — async function getSkills(): Promise<SkillDto[]>
- getGraph · function · L179-L182 — async function getGraph(): Promise<WorldGraphDto>
