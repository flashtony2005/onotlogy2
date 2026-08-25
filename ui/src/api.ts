// REST 客户端：与 api-agent-trace.md 对齐（{ code, msg, data } 信封，前缀 /api/v1/wb/agent）。
// BASE 留空 → 同源（生产由 Rust 服务托管，开发由 Vite 代理转发）。
// V0.7：除 /auth/login 外全部请求注入 `Authorization: Bearer <token>`；401 抛 AuthError 由 UI 跳登录。

import { getToken } from './auth'
import type {
  ApiResponse,
  AuthUserDto,
  UserDto,
  PluginDto,
  RawSkillDto,
  RunStartDto,
  AuditQueryDto,
  DomainPackDto,
  DomainPackInstallDto,
  DomainPackOperationsDto,
  AutomationJobDto,
  TraceEventDto,
  AutomationStatusDto,
  AutomationPackStatusDto,
  DomainPackOperation,
  DomainPackRuntimeDto,
  DomainPackRuntimeUpdateDto,
  StrategyRegistryDto,
  AuditViewDto,
  FabricRequestDto,
  FabricSubmissionDto,
  PendingApprovalDto,
  ApprovalDecisionDto,
  ProjectCreateDto,
  ProjectDto,
  BusinessGoalDto,
  BusinessPathDto,
  BusinessRecommendationDto,
  BusinessRunDto,
  ProjectEventDto,
  SessionDto,
  SessionSummaryDto,
  SkillDto,
  WorldGraphDto,
  WorldQueryDto,
  WorldViewDto,
  WorkerDto,
  KnowledgeVersionDto,
  KnowledgeDiagnosisDto,
  OntologyEvolutionRecordDto,
  OntologyProposalDto,
} from './types'

/** 401（令牌缺失/过期/禁用）→ UI 据此清令牌并跳登录。 */
export class AuthError extends Error {
  constructor(msg = '登录已过期，请重新登录') {
    super(msg)
    this.name = 'AuthError'
  }
}

/// 请求超时（后端 hang 时避免 UI 永久 pending）。
const FETCH_TIMEOUT_MS = 30_000

/** 带超时的 fetch：AbortController 兜底，超时抛可读错误。 */
async function fetchWithTimeout(input: string, init?: RequestInit): Promise<Response> {
  const ctrl = new AbortController()
  const timer = setTimeout(() => ctrl.abort(), FETCH_TIMEOUT_MS)
  try {
    return await fetch(input, { ...init, signal: ctrl.signal })
  } catch (e) {
    if (e instanceof DOMException && e.name === 'AbortError') {
      throw new Error(`请求超时（>${FETCH_TIMEOUT_MS / 1000}s）：${input}`)
    }
    throw e
  } finally {
    clearTimeout(timer)
  }
}

async function unwrap<T>(res: Response): Promise<T> {
  // U3：先判 HTTP 状态；响应体非 JSON（网关/500 HTML）时兜底可读错误，不抛 SyntaxError。
  // B3：非 2xx 也尝试解析信封的 msg（如登录限流 429 的「请 X 分钟后再试」），丢失可读信息。
  if (!res.ok) {
    let msg = `请求失败 (HTTP ${res.status})`
    try {
      const j = (await res.json()) as ApiResponse<unknown>
      if (j.msg) msg = j.msg
    } catch {
      /* 非 JSON 响应：保留通用错误 */
    }
    throw new Error(msg)
  }
  let json: ApiResponse<T>
  try {
    json = (await res.json()) as ApiResponse<T>
  } catch {
    throw new Error('服务异常：响应不是有效 JSON')
  }
  if (json.code !== 0 || !json.data) {
    throw new Error(json.msg || `请求失败 (code=${json.code})`)
  }
  return json.data
}

/** 带 Bearer 令牌的请求；401 统一抛 AuthError（不在各调用点重复判断）。 */
async function authFetch(input: string, init?: RequestInit): Promise<Response> {
  const token = getToken()
  const headers = new Headers(init?.headers)
  if (token) headers.set('Authorization', `Bearer ${token}`)
  const res = await fetchWithTimeout(input, { ...init, headers })
  if (res.status === 401) throw new AuthError()
  return res
}

// ── V0.7 认证 ───────────────────────────────────────────────────────────────

/** `POST /api/v1/wb/auth/login`：公开端点，换取不透明会话令牌。 */
export async function postLogin(
  username: string,
  password: string,
): Promise<{ token: string; user: AuthUserDto }> {
  const res = await fetch('/api/v1/wb/auth/login', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ username, password }),
  })
  return unwrap<{ token: string; user: AuthUserDto }>(res)
}

/** `GET /api/v1/wb/auth/me`：恢复会话（刷新/重开页面后校验令牌仍有效）。 */
export async function getMe(): Promise<AuthUserDto> {
  const res = await authFetch('/api/v1/wb/auth/me')
  return unwrap<AuthUserDto>(res)
}

/** `POST /api/v1/wb/auth/logout`：撤销当前会话令牌。 */
export async function postLogout(): Promise<void> {
  const res = await authFetch('/api/v1/wb/auth/logout', { method: 'POST' })
  await unwrap<{ loggedOut: boolean }>(res)
}

// ── 用户管理（仅 Developer）─────────────────────────────────────────────────

export async function getUsers(): Promise<UserDto[]> {
  const res = await authFetch('/api/v1/wb/users')
  return unwrap<UserDto[]>(res)
}

export async function createUser(req: {
  username: string
  password: string
  role: string
  displayName?: string
}): Promise<UserDto> {
  const res = await authFetch('/api/v1/wb/users', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  })
  return unwrap<UserDto>(res)
}

export async function resetUserPassword(userId: string, password: string): Promise<{ updated: boolean }> {
  const res = await authFetch(`/api/v1/wb/users/${encodeURIComponent(userId)}/password`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ password }),
  })
  return unwrap<{ updated: boolean }>(res)
}

export async function setUserDisabled(userId: string, disabled: boolean): Promise<{ disabled: boolean }> {
  const res = await authFetch(`/api/v1/wb/users/${encodeURIComponent(userId)}/disabled`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ disabled }),
  })
  return unwrap<{ disabled: boolean }>(res)
}

export async function setUserRole(userId: string, role: string): Promise<{ role: string }> {
  const res = await authFetch(`/api/v1/wb/users/${encodeURIComponent(userId)}/role`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ role }),
  })
  return unwrap<{ role: string }>(res)
}

// ── Agent 观察 API ──────────────────────────────────────────────────────────

export async function postRun(agentId: string, goal: string): Promise<RunStartDto> {
  const res = await authFetch('/api/v1/wb/agent/run', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ agentId, goal }),
  })
  return unwrap<RunStartDto>(res)
}

export async function getSkillRaw(skillId: string): Promise<RawSkillDto> {
  const res = await authFetch(`/api/v1/v2/skills/${skillId}/raw`)
  if (!res.ok) throw new Error(`skill raw ${res.status}`)
  return await res.json()
}

export async function importSkill(
  name: string,
  content: string,
  knownCapabilities: string[],
): Promise<SkillDto> {
  const res = await authFetch('/api/v1/v2/skills/import', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name, content, known_capabilities: knownCapabilities }),
  })
  if (!res.ok) throw new Error(`import ${res.status}`)
  return await res.json()
}

export async function getPlugins(): Promise<PluginDto[]> {
  const res = await authFetch('/api/v1/v2/plugins')
  if (!res.ok) throw new Error(`plugins ${res.status}`)
  return await res.json()
}

export async function getWorkers(): Promise<WorkerDto[]> {
  const res = await authFetch('/api/v1/v2/workers')
  if (!res.ok) throw new Error(`workers ${res.status}`)
  return await res.json()
}

export async function getSessions(): Promise<SessionSummaryDto[]> {
  const res = await authFetch('/api/v1/v2/sessions')
  if (!res.ok) throw new Error(`sessions ${res.status}`)
  return await res.json()
}

export async function getSession(sessionId: string): Promise<SessionDto> {
  const res = await authFetch(`/api/v1/v2/sessions/${sessionId}`)
  if (!res.ok) throw new Error(`session ${res.status}`)
  return await res.json()
}

export async function createSession(sessionId: string): Promise<SessionDto> {
  const res = await authFetch(`/api/v1/v2/sessions/${sessionId}/create`, {
    method: 'POST',
  })
  if (!res.ok) throw new Error(`session create ${res.status}`)
  return await res.json()
}

/// `GET /api/v1/wb/graph`：世界对象-关系图（World 页数据源）。
/** `GET /api/v1/v2/skills`：已导入 Skill 列表（V2 面板数据源）。 */
export async function getSkills(): Promise<SkillDto[]> {
  const res = await authFetch('/api/v1/v2/skills')
  if (!res.ok) throw new Error(`skills ${res.status}`)
  return await res.json()
}

/** `GET /api/v1/domain-packs`：发现、校验并汇总本地业务包。 */
export async function getDomainPacks(): Promise<DomainPackDto[]> {
  const res = await authFetch('/api/v1/domain-packs')
  return unwrap<DomainPackDto[]>(res)
}

/** `POST /api/v1/domain-packs/:id/install`：Developer 安装业务包。 */
export async function installDomainPack(id: string): Promise<DomainPackInstallDto> {
  const res = await authFetch(`/api/v1/domain-packs/${encodeURIComponent(id)}/install`, {
    method: 'POST',
  })
  return unwrap<DomainPackInstallDto>(res)
}

export async function getDomainPackRuntime(id: string): Promise<DomainPackRuntimeDto> {
  const res = await authFetch(`/api/v1/domain-packs/${encodeURIComponent(id)}/runtime`)
  return unwrap<DomainPackRuntimeDto>(res)
}

export async function updateDomainPackRuntime(
  id: string,
  runtime: DomainPackRuntimeUpdateDto,
): Promise<DomainPackRuntimeDto> {
  const res = await authFetch(`/api/v1/domain-packs/${encodeURIComponent(id)}/runtime`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(runtime),
  })
  return unwrap<DomainPackRuntimeDto>(res)
}

export async function setDomainPackEnabled(id: string, enabled: boolean): Promise<DomainPackRuntimeDto> {
  const action = enabled ? 'enable' : 'disable'
  const res = await authFetch(`/api/v1/domain-packs/${encodeURIComponent(id)}/${action}`, {
    method: 'POST',
  })
  return unwrap<DomainPackRuntimeDto>(res)
}

export async function getDomainPackOperations(id: string): Promise<DomainPackOperationsDto> {
  const res = await authFetch(`/api/v1/domain-packs/${encodeURIComponent(id)}/operations`)
  return unwrap<DomainPackOperationsDto>(res)
}

export async function getDomainPackStrategies(id: string): Promise<StrategyRegistryDto[]> {
  const res = await authFetch(`/api/v1/domain-packs/${encodeURIComponent(id)}/strategies`)
  return unwrap<StrategyRegistryDto[]>(res)
}

/** Phase 2.5：自动运行引擎任务观察与手动触发。 */
export async function getAutomationJobs(): Promise<AutomationStatusDto> {
  const res = await authFetch('/api/v1/automation/jobs')
  return unwrap<AutomationStatusDto>(res)
}

export async function getDomainPackAutomationStatus(id: string): Promise<AutomationPackStatusDto> {
  const res = await authFetch(`/api/v1/domain-packs/${encodeURIComponent(id)}/automation-status`)
  return unwrap<AutomationPackStatusDto>(res)
}

export async function getTrace(id: string): Promise<TraceEventDto> {
  const res = await authFetch(`/api/v1/wb/agent/trace/${encodeURIComponent(id)}`)
  return unwrap<TraceEventDto>(res)
}

export async function getAutomationJob(id: string): Promise<AutomationJobDto> {
  const res = await authFetch(`/api/v1/automation/jobs/${encodeURIComponent(id)}`)
  return unwrap<AutomationJobDto>(res)
}

export async function cancelAutomationJob(id: string): Promise<AutomationJobDto> {
  const res = await authFetch(`/api/v1/automation/jobs/${encodeURIComponent(id)}/cancel`, {
    method: 'POST',
  })
  return unwrap<AutomationJobDto>(res)
}

export async function runDomainPack(id: string): Promise<AutomationJobDto> {
  const res = await authFetch(`/api/v1/domain-packs/${encodeURIComponent(id)}/run`, {
    method: 'POST',
  })
  return unwrap<AutomationJobDto>(res)
}

export async function scanDomainPack(id: string): Promise<AutomationJobDto[]> {
  const res = await authFetch(`/api/v1/domain-packs/${encodeURIComponent(id)}/scan`, {
    method: 'POST',
  })
  return unwrap<AutomationJobDto[]>(res)
}

export async function runDomainPackOperation(
  id: string,
  operation: DomainPackOperation,
): Promise<AutomationJobDto> {
  const res = await authFetch(
    `/api/v1/domain-packs/${encodeURIComponent(id)}/operations/${encodeURIComponent(operation)}/run`,
    { method: 'POST' },
  )
  return unwrap<AutomationJobDto>(res)
}

export async function getGraph(): Promise<WorldGraphDto> {
  const res = await authFetch('/api/v1/wb/graph')
  return unwrap<WorldGraphDto>(res)
}

/// `GET /api/v1/knowledge/ontology/current`：当前不可变知识本体版本。
export async function getKnowledgeCurrent(): Promise<KnowledgeVersionDto> {
  const res = await authFetch('/api/v1/knowledge/ontology/current')
  return unwrap<KnowledgeVersionDto>(res)
}

/// `GET /api/v1/knowledge/diagnosis`：TBox + live ABox 一次性推理诊断。
export async function diagnoseKnowledge(): Promise<KnowledgeDiagnosisDto> {
  const res = await authFetch('/api/v1/knowledge/diagnosis')
  return unwrap<KnowledgeDiagnosisDto>(res)
}

/// `GET /api/v1/knowledge/ontology/versions`：不可变本体版本谱系。
export async function getKnowledgeVersions(): Promise<KnowledgeVersionDto[]> {
  const res = await authFetch('/api/v1/knowledge/ontology/versions')
  const page = await unwrap<{ total: number; list: KnowledgeVersionDto[] }>(res)
  return page.list
}

/// `GET /api/v1/knowledge/proposals`：持久化治理提案与评审状态。
export async function getKnowledgeProposals(): Promise<OntologyEvolutionRecordDto[]> {
  const res = await authFetch('/api/v1/knowledge/proposals')
  const page = await unwrap<{ total: number; list: OntologyEvolutionRecordDto[] }>(res)
  return page.list
}

/// `POST /api/v1/knowledge/proposals`：创建待评审的持久化本体演化提案。
export async function createKnowledgeProposal(
  sourceVersionId: string,
  proposal: OntologyProposalDto,
  runtimeIr?: unknown,
): Promise<OntologyEvolutionRecordDto> {
  const res = await authFetch('/api/v1/knowledge/proposals', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ sourceVersionId, proposal, runtimeIr: runtimeIr ?? null }),
  })
  return unwrap<OntologyEvolutionRecordDto>(res)
}

/// `POST /api/v1/knowledge/proposals/:id/review`：具名人工评审。
export async function reviewKnowledgeProposal(
  id: string,
  approve: boolean,
  reason?: string,
): Promise<OntologyEvolutionRecordDto> {
  const res = await authFetch(
    `/api/v1/knowledge/proposals/${encodeURIComponent(id)}/review`,
    {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ approve, reason }),
    },
  )
  return unwrap<OntologyEvolutionRecordDto>(res)
}

/// `POST /api/v1/knowledge/proposals/:id/apply`：创建不可变目标版本并热更新运行时。
export async function applyKnowledgeProposal(
  id: string,
  targetVersionId: string,
): Promise<{ proposal: OntologyEvolutionRecordDto; version: KnowledgeVersionDto }> {
  const res = await authFetch(
    `/api/v1/knowledge/proposals/${encodeURIComponent(id)}/apply`,
    {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ targetVersionId }),
    },
  )
  return unwrap<{ proposal: OntologyEvolutionRecordDto; version: KnowledgeVersionDto }>(res)
}

/// `POST /api/v1/knowledge/rollback`：把回滚也变成受评审的 Replace 提案。
export async function proposeKnowledgeRollback(
  proposalId: string,
  targetVersionId: string,
): Promise<OntologyEvolutionRecordDto> {
  const res = await authFetch('/api/v1/knowledge/rollback', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ proposalId, targetVersionId }),
  })
  return unwrap<OntologyEvolutionRecordDto>(res)
}

/// REQ-018：列出自动放行审计（wb_approval_audit，可追溯至 LearnedKnowledge + run）。

// ── Phase 7 Facade Transport（POST /api/v1/facade/*；单一语义源）──

export async function postWorldQuery(req: WorldQueryDto): Promise<WorldViewDto> {
  const r = await authFetch('/api/v1/facade/world/query', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  })
  const json: ApiResponse<WorldViewDto> = await r.json()
  if (json.code !== 0 || !json.data) {
    throw new Error(json.msg || `世界查询失败 (code=${json.code})`)
  }
  return json.data
}

export async function postAuditQuery(req: AuditQueryDto): Promise<AuditViewDto> {
  const r = await authFetch('/api/v1/facade/audit/query', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  })
  const json: ApiResponse<AuditViewDto> = await r.json()
  if (json.code !== 0 || !json.data) {
    throw new Error(json.msg || `审计查询失败 (code=${json.code})`)
  }
  return json.data
}

export async function postProjectList(): Promise<ProjectDto[]> {
  const r = await authFetch('/api/v1/facade/project/list', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: '{}',
  })
  const json: ApiResponse<ProjectDto[]> = await r.json()
  if (json.code !== 0 || !json.data) throw new Error(json.msg || '项目列表失败')
  return json.data
}

export async function postProjectSeed(): Promise<{ events: number }> {
  const r = await authFetch('/api/v1/facade/project/seed', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: '{}',
  })
  const json: ApiResponse<{ events: number }> = await r.json()
  if (json.code !== 0 || !json.data) throw new Error(json.msg || 'seed 失败')
  return json.data
}

export async function postProjectCreate(req: ProjectCreateDto): Promise<ProjectDto> {
  const r = await authFetch('/api/v1/facade/project/create', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  })
  const json: ApiResponse<ProjectDto> = await r.json()
  if (json.code !== 0 || !json.data) throw new Error(json.msg || '项目创建失败')
  return json.data
}

/** `POST /api/v1/facade/project/event`：给项目世界加对象（CreateObject + 可选 root --edge）。 */
export async function postProjectEvent(req: ProjectEventDto): Promise<{ ok: boolean }> {
  const r = await authFetch('/api/v1/facade/project/event', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  })
  const json: ApiResponse<{ ok: boolean }> = await r.json()
  if (json.code !== 0 || !json.data) throw new Error(json.msg || '添加项目对象失败')
  return json.data
}

/** `POST /api/v1/facade/project/deps`：依赖图 seed（crates 依赖 → depends_on 关系入项目世界）。 */
export async function postProjectDeps(): Promise<{ relations: number }> {
  const r = await authFetch('/api/v1/facade/project/deps', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: '{}',
  })
  const json: ApiResponse<{ relations: number }> = await r.json()
  if (json.code !== 0 || !json.data) throw new Error(json.msg || '依赖图 seed 失败')
  return json.data
}

export async function postFabricSubmit(req: FabricRequestDto): Promise<FabricSubmissionDto> {
  const r = await authFetch('/api/v1/facade/fabric/submit', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  })
  const json: ApiResponse<FabricSubmissionDto> = await r.json()
  if (json.code !== 0 || !json.data) {
    throw new Error(json.msg || `fabric 提交失败 (code=${json.code})`)
  }
  return json.data
}

// ── T1 Bridge 审批裁决（GET /run/:run_id/approvals + POST /run/:run_id/approval/:req_id）──

/// `GET /api/v1/wb/agent/run/:run_id/approvals`：列出某 run 的挂起审批（需 Operator 及以上）。
export async function getRunApprovals(runId: string): Promise<PendingApprovalDto[]> {
  const r = await authFetch(`/api/v1/wb/agent/run/${encodeURIComponent(runId)}/approvals`)
  return unwrap<PendingApprovalDto[]>(r)
}

/// `POST /api/v1/wb/agent/run/:run_id/approval/:req_id`：裁决一条挂起审批，唤醒执行（需 Operator 及以上）。
/// `approve=true` → decision=approve（放行续跑）；`approve=false` → decision=reject（终止）。
export async function postRunApprovalDecision(
  runId: string,
  reqId: string,
  approve: boolean,
): Promise<ApprovalDecisionDto> {
  const r = await authFetch(
    `/api/v1/wb/agent/run/${encodeURIComponent(runId)}/approval/${encodeURIComponent(reqId)}`,
    {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ decision: approve ? 'approve' : 'reject' }),
    },
  )
  return unwrap<ApprovalDecisionDto>(r)
}

// ── Business Loop：企业状态 → 机会 → 组合选择 → 执行反馈 ──

export async function getBusinessGoals(projectId: string): Promise<BusinessGoalDto[]> {
  const res = await authFetch(`/api/v1/business/goals?projectId=${encodeURIComponent(projectId)}`)
  return unwrap<BusinessGoalDto[]>(res)
}

export async function getBusinessRecommendations(projectId: string): Promise<BusinessRecommendationDto[]> {
  const res = await authFetch(`/api/v1/business/recommendations?projectId=${encodeURIComponent(projectId)}`)
  return unwrap<BusinessRecommendationDto[]>(res)
}

export async function getBusinessPortfolio(projectId: string): Promise<BusinessPathDto[]> {
  const res = await authFetch(`/api/v1/business/portfolio?projectId=${encodeURIComponent(projectId)}`)
  return unwrap<BusinessPathDto[]>(res)
}

export async function runBusinessNext(projectId: string): Promise<BusinessRunDto> {
  const r = await authFetch('/api/v1/business/run-next', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ projectId }),
  })
  return unwrap<BusinessRunDto>(r)
}
