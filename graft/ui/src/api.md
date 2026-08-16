# ui/src/api.ts · [[learn-permission-invariant]] [[ui-dashboard]]

REST client module for the agent backend, wrapping all API calls in the {code,msg,data} envelope with /api/v1/wb/agent prefix.

- AuthError · class · L28-L33 — class AuthError extends Error
- constructor · method · L29-L32 — constructor(msg = '登录已过期，请重新登录')
- fetchWithTimeout · function · L39-L52 — async function fetchWithTimeout(input: string, init?: RequestInit): Promise<Response>
- unwrap · function · L54-L77 — Unwraps the backend envelope, throwing on non-zero code or missing data so callers get typed payloads or a clear error.
- authFetch · function · L80-L87 — async function authFetch(input: string, init?: RequestInit): Promise<Response>
- postLogin · function · L92-L102 — async function postLogin( username: string, password: string, ): Promise<{ token: string; user: AuthUserDto }>
- getMe · function · L105-L108 — async function getMe(): Promise<AuthUserDto>
- postLogout · function · L111-L114 — async function postLogout(): Promise<void>
- postRun · function · L118-L125 — Starts a new agent run by POSTing the agent id and goal to the run endpoint.
- getSkillRaw · function · L127-L131 — async function getSkillRaw(skillId: string): Promise<RawSkillDto>
- importSkill · function · L133-L145 — async function importSkill( name: string, content: string, knownCapabilities: string[], ): Promise<SkillDto>
- getPlugins · function · L147-L151 — async function getPlugins(): Promise<PluginDto[]>
- getWorkers · function · L153-L157 — async function getWorkers(): Promise<WorkerDto[]>
- getSessions · function · L159-L163 — async function getSessions(): Promise<SessionSummaryDto[]>
- getSession · function · L165-L169 — async function getSession(sessionId: string): Promise<SessionDto>
- createSession · function · L171-L177 — async function createSession(sessionId: string): Promise<SessionDto>
- getSkills · function · L181-L185 — async function getSkills(): Promise<SkillDto[]>
- getGraph · function · L187-L190 — async function getGraph(): Promise<WorldGraphDto>
- postWorldQuery · function · L196-L207 — async function postWorldQuery(req: WorldQueryDto): Promise<WorldViewDto>
- postAuditQuery · function · L209-L220 — async function postAuditQuery(req: AuditQueryDto): Promise<AuditViewDto>
- postProjectList · function · L222-L231 — async function postProjectList(): Promise<ProjectDto[]>
- postProjectCreate · function · L233-L242 — async function postProjectCreate(req: ProjectCreateDto): Promise<ProjectDto>
- postFabricSubmit · function · L244-L255 — async function postFabricSubmit(req: FabricRequestDto): Promise<FabricSubmissionDto>
