// REST 客户端：与 api-agent-trace.md 对齐（{ code, msg, data } 信封，前缀 /api/v1/wb/agent）。
// BASE 留空 → 同源（生产由 Rust 服务托管，开发由 Vite 代理转发）。
// V0.7：除 /auth/login 外全部请求注入 `Authorization: Bearer <token>`；401 抛 AuthError 由 UI 跳登录。

import { getToken } from './auth'
import type {
  ApiResponse,
  ApprovalAuditDto,
  AuthUserDto,
  EvolutionStateDto,
  MemoryListDto,
  PluginDto,
  RawSkillDto,
  RunStartDto,
  SessionDto,
  SessionSummaryDto,
  SkillDto,
  TaskListDto,
  WorldGraphDto,
  WorkerDto,
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

// ── Agent 观察 API ──────────────────────────────────────────────────────────

export async function postRun(agentId: string, goal: string): Promise<RunStartDto> {
  const res = await authFetch('/api/v1/wb/agent/run', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ agentId, goal }),
  })
  return unwrap<RunStartDto>(res)
}

/// `GET /api/v1/wb/agent/memory`：记忆存储全量（V0.5 Memory 视图数据源）。
export async function getMemory(): Promise<MemoryListDto> {
  const res = await authFetch('/api/v1/wb/agent/memory')
  return unwrap<MemoryListDto>(res)
}

/// `GET /api/v1/wb/agent/tasks`：RetentionTask 状态列表。
export async function getTasks(): Promise<TaskListDto> {
  const res = await authFetch('/api/v1/wb/agent/tasks')
  return unwrap<TaskListDto>(res)
}

/// `GET /api/v1/wb/agent/evolution`：Evolution 聚合状态（BL-003 · REQ-017）。
/// 仅暴露知识，不含任何改写 Policy / 绕过 Human Approval 的能力（Learn≠Permission）。
// ── Phase 10 V2 认知面板 API（带 Bearer 令牌；401 抛 AuthError）─────────
export async function getSkills(): Promise<SkillDto[]> {
  const res = await authFetch('/api/v1/v2/skills')
  if (!res.ok) throw new Error(`skills ${res.status}`)
  return await res.json()
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

export async function getEvolution(): Promise<EvolutionStateDto> {
  const res = await authFetch('/api/v1/wb/agent/evolution')
  return unwrap<EvolutionStateDto>(res)
}

/// `GET /api/v1/wb/graph`：世界对象-关系图（World 页数据源）。
export async function getGraph(): Promise<WorldGraphDto> {
  const res = await authFetch('/api/v1/wb/graph')
  return unwrap<WorldGraphDto>(res)
}

/// REQ-018：切换某条 LearnedKnowledge 的自动放行资格（可撤销，幂等）。
/// `enabled=true` 恢复、`false` 撤销（下次该 action 回退 Human Approval）。
export async function postAutoApproveToggle(
  id: string,
  enabled: boolean,
): Promise<{ id: string; autoApproveEnabled: boolean }> {
  const res = await authFetch(`/api/v1/wb/agent/evolution/learned/${id}/auto-approve`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ enabled }),
  })
  return unwrap<{ id: string; autoApproveEnabled: boolean }>(res)
}

/// REQ-018：列出自动放行审计（wb_approval_audit，可追溯至 LearnedKnowledge + run）。
export async function getApprovalAudit(): Promise<ApprovalAuditDto[]> {
  const res = await authFetch('/api/v1/wb/agent/evolution/audit')
  return unwrap<ApprovalAuditDto[]>(res)
}
