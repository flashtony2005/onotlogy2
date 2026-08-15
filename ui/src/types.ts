// 与 src/trace.rs / docs/05_接口文档/api-agent-trace.md 4.1 同源（camelCase，单一真相源）

export type ViewKind = 'overview' | 'executive' | 'operator' | 'developer' | 'memory' | 'evolution' | 'tasks' | 'v2'

/// V0.7 认证用户（对应 server `GET /api/v1/wb/auth/me` / 登录响应 user）。
export interface AuthUserDto {
  username: string
  role: string
}

export interface TaskRecord {
  id: string
  title: string
  customer: string
  status: string
  priority: string
}

export interface TaskListDto {
  total: number
  list: TaskRecord[]
}

export interface TraceEventDto {
  id: string
  kind: string
  eventType: string
  objectRefs?: string[]
  runId?: string
  agentId?: string
  reasoning?: string
  input?: unknown
  output?: unknown
  confidence?: number
  timestamp: string
}

export interface TimelineEvent {
  id: string
  eventType: string
  timestamp: string
  confidence?: number
  objectRefs?: string[]
  reasoning?: string
  input?: unknown
  output?: unknown
}

export interface DecisionSummary {
  mission: string
  decision: string
  risk: string
}

export interface RunTimeline {
  runId: string
  agentId: string
  status: string
  progress: number
  humanApproval: string
  events: TimelineEvent[]
  decision?: DecisionSummary
}

export interface RunStartDto {
  runId: string
  status: string
  streamUrl: string
}

export interface ApiResponse<T> {
  code: number
  msg: string
  data?: T
}

/// 一条记忆记录（对应后端 wb_memory / GET /memory）。
export interface MemoryRecord {
  label: string
  payload: unknown
  hits: number
  createdAt: string
  updatedAt: string
}

export interface MemoryListDto {
  total: number
  list: MemoryRecord[]
}

/// Evolution 产物类型（对应 src/evolution.rs；由 GET /evolution 暴露）。
/// 全程可追溯：Pattern.runIds → Proposal.patternLabel → Learned.proposalId。
export interface PatternDto {
  label: string
  action: string
  support: number
  confidence: number
  runIds: string[]
}

export interface ProposalDto {
  id: string
  kind: string
  target: string
  confidence: number
  status: 'Proposed' | 'Validated' | 'Rejected'
  patternLabel: string
  runIds: string[]
}

export interface LearnedDto {
  id: string
  kind: string
  action: string
  confidence: number
  proposalId: string
  runIds: string[]
  /// REQ-018 自动放行资格（可撤销）
  autoApproveEnabled: boolean
  createdAt: string
}

/// REQ-018 自动放行审计（对应 wb_approval_audit）
export interface ApprovalAuditDto {
  id: string
  runId: string
  action: string
  learnedId: string
  decision: 'auto_approve' | 'auto_approve_enabled' | 'revoke_disabled'
  reason: string
  createdAt: string
}

// ── Phase 10 V2 认知面板（Skill / Plugin / Session）────────────────────
export interface SkillDto {
  id: string
  name: string
  version: string
  skill_type: string
  status: string
  goal: string
  capabilities: string[]
}

export interface PluginDto {
  id: string
  name: string
  status: string
  runtime: string
  endpoint?: string | null
  capabilities: string[]
}

export interface SessionEventDto {
  seq: number
  event_type: string
  timestamp: number
  data: Record<string, unknown>
}

export interface SessionSummaryDto {
  session_id: string
  events: number
}

export interface SessionDto {
  session_id: string
  events: SessionEventDto[]
}

export interface EvolutionStateDto {
  patterns: PatternDto[]
  proposals: ProposalDto[]
  learned: LearnedDto[]
}
