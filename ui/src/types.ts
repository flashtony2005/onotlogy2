// 与 src/trace.rs / docs/05_接口文档/api-agent-trace.md 4.1 同源（camelCase，单一真相源）

export type ViewKind =
  | 'overview'
  | 'world'
  | 'runs'
  | 'workers'
  | 'learning'
  | 'governance'
  | 'fabric'
  | 'developer'
  // 旧视图保留给后端 timeline 视图钳制与兼容（新菜单不再直接暴露）。
  | 'executive'
  | 'operator'
  | 'v2'

/// V0.7 认证用户（对应 server `GET /api/v1/wb/auth/me` / 登录响应 user）。
export interface AuthUserDto {
  username: string
  role: string
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


/// 已导入 Skill（V2 面板数据源：/api/v1/v2/skills）。
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

export interface WorkerDto {
  id: string
  worker_type: string
  capabilities: string[]
  execution: string
  quality: number
  cost: number
  status: string
}

export interface RawSkillDto {
  id: string
  name: string
  source_type: string
  content_hash: string
  content: string
}

export interface SessionSummaryDto {
  session_id: string
  events: number
}

export interface SessionDto {
  session_id: string
  events: SessionEventDto[]
}

// ── World 视图（GET /api/v1/wb/graph）───────────────────────────
export interface WorldNodeDto {
  id: string
  type: string
  data?: unknown
}

export interface WorldEdgeDto {
  id: string
  source: string
  target: string
  type: string
  data?: unknown
}

export interface WorldGraphDto {
  nodes: WorldNodeDto[]
  edges: WorldEdgeDto[]
}


// ── Phase 7 Facade DTO（对应 server /api/v1/facade/*；camelCase wire）──

export interface WorldRelationDto {
  source: string
  predicate: string
  target: string
  validFrom: number
  validUntil: number | null
}

export interface WorldViewDto {
  relations: WorldRelationDto[]
  total: number
  at: number
}

export interface WorldQueryDto {
  objectRef?: string
  predicate?: string
  limit?: number
}

export interface AuditEventDto {
  id: string
  eventType: string
  refs: string[]
  payload: unknown
  timestamp: string
}

export interface AuditViewDto {
  events: AuditEventDto[]
  total: number
}

export interface AuditQueryDto {
  runId?: string
  agentId?: string
  eventType?: string
  limit?: number
}

export interface FabricRequestDto {
  sessionId: string
  sender: string
  intent: string
  payload: unknown
}

export interface FabricSubmissionDto {
  // AgentMessage wire 为 snake_case（契约示例）；此处照实际字段声明
  result: {
    id: string
    session_id: string
    sender: string
    receiver: string
    kind: string
    intent: string | null
    payload: Record<string, unknown>
    capability: string | null
    correlation_id: string | null
    causation_id: string | null
    timestamp: number
  }
}
