# ui/src/api.ts · [[learn-permission-invariant]] [[ui-dashboard]]

REST client module for the agent backend, wrapping all API calls in the {code,msg,data} envelope with /api/v1/wb/agent prefix.

- AuthError · class · L26-L31 — class AuthError extends Error
- constructor · method · L27-L30 — constructor(msg = '登录已过期，请重新登录')
- fetchWithTimeout · function · L37-L50 — async function fetchWithTimeout(input: string, init?: RequestInit): Promise<Response>
- unwrap · function · L52-L75 — Unwraps the backend envelope, throwing on non-zero code or missing data so callers get typed payloads or a clear error.
- authFetch · function · L78-L85 — async function authFetch(input: string, init?: RequestInit): Promise<Response>
- postLogin · function · L90-L100 — async function postLogin( username: string, password: string, ): Promise<{ token: string; user: AuthUserDto }>
- getMe · function · L103-L106 — async function getMe(): Promise<AuthUserDto>
- postLogout · function · L109-L112 — async function postLogout(): Promise<void>
- postRun · function · L116-L123 — Starts a new agent run by POSTing the agent id and goal to the run endpoint.
- getSkillRaw · function · L125-L129 — async function getSkillRaw(skillId: string): Promise<RawSkillDto>
- importSkill · function · L131-L143 — async function importSkill( name: string, content: string, knownCapabilities: string[], ): Promise<SkillDto>
- getPlugins · function · L145-L149 — async function getPlugins(): Promise<PluginDto[]>
- getWorkers · function · L151-L155 — async function getWorkers(): Promise<WorkerDto[]>
- getSessions · function · L157-L161 — async function getSessions(): Promise<SessionSummaryDto[]>
- getSession · function · L163-L167 — async function getSession(sessionId: string): Promise<SessionDto>
- createSession · function · L169-L175 — async function createSession(sessionId: string): Promise<SessionDto>
- getSkills · function · L179-L183 — async function getSkills(): Promise<SkillDto[]>
- getGraph · function · L185-L188 — async function getGraph(): Promise<WorldGraphDto>
- postWorldQuery · function · L194-L205 — async function postWorldQuery(req: WorldQueryDto): Promise<WorldViewDto>
- postAuditQuery · function · L207-L218 — async function postAuditQuery(req: AuditQueryDto): Promise<AuditViewDto>
- postFabricSubmit · function · L220-L231 — async function postFabricSubmit(req: FabricRequestDto): Promise<FabricSubmissionDto>
