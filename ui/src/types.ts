// 与 src/trace.rs / docs/05_接口文档/api-agent-trace.md 4.1 同源（camelCase，单一真相源）

export type ViewKind = 'executive' | 'operator' | 'developer'

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
