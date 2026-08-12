// 三层视图裁剪：与 src/trace.rs::build_timeline 同源（REQ-009 / agent-observation-design.md 4.5）。
// Executive 隐藏思考、Operator 仅 8 步 Pipeline 骨架、Developer 全量 IO。
// 与 REST /timeline?view= 结果一致，避免 WS 实时流与离线查询漂移。

import type {
  DecisionSummary,
  RunTimeline,
  TimelineEvent,
  TraceEventDto,
  ViewKind,
} from './types'

const COGNITIVE_STEPS = [
  'Goal',
  'Intent',
  'OntologyQuery',
  'Reasoning',
  'Planning',
  'PolicyCheck',
  'Action',
  'MemoryUpdate',
]

function toTimelineEvent(e: TraceEventDto, detail: boolean): TimelineEvent {
  return {
    id: e.id,
    eventType: e.eventType,
    timestamp: e.timestamp,
    confidence: e.confidence,
    objectRefs: e.objectRefs,
    reasoning: detail ? e.reasoning : undefined,
    input: detail ? e.input : undefined,
    output: detail ? e.output : undefined,
  }
}

function asString(v: unknown): string | undefined {
  if (v == null) return undefined
  return typeof v === 'string' ? v : JSON.stringify(v)
}

export function buildTimeline(
  events: TraceEventDto[],
  runId: string,
  agentId: string,
  view: ViewKind,
): RunTimeline {
  const status = events.some((e) => e.eventType === 'MemoryUpdate') ? 'Completed' : 'Running'
  const stepCount = COGNITIVE_STEPS.filter((s) =>
    events.some((e) => e.eventType === s),
  ).length
  const progress = Math.round((stepCount / COGNITIVE_STEPS.length) * 100)
  const denied = events.some(
    (e) => e.eventType === 'PolicyCheck' && JSON.stringify(e.output) === '"Deny"',
  )
  const humanApproval = denied ? 'Required' : 'None'

  let tlEvents: TimelineEvent[] = []
  let decision: DecisionSummary | undefined

  if (view === 'memory') {
    // Memory 视图不渲染认知时间线（由 App 单独渲染记忆列表）
    return { runId, agentId, status, progress, humanApproval, events: [], decision: undefined }
  } else if (view === 'executive') {
    // 仅 Mission(Goal) + Decision/Outcome(Action)，隐藏思考细节
    const kept = events
      .filter((e) => e.eventType === 'Goal' || e.eventType === 'Action')
      .map((e) => toTimelineEvent(e, false))
    const goalEv = events.find((e) => e.eventType === 'Goal')
    const mission = asString((goalEv?.input as { goal?: unknown } | undefined)?.goal) ?? ''
    const actionEv = events.find((e) => e.eventType === 'Action')
    const decisionText =
      (actionEv && asString(actionEv.output)) ??
      actionEv?.reasoning ??
      '无明确决策'
    const risk = events
      .filter((e) => e.eventType === 'Reasoning')
      .map((e) => e.confidence ?? 0)
      .reduce((a, b) => Math.max(a, b), 0)
    decision = { mission, decision: decisionText, risk: risk.toFixed(2) }
    tlEvents = kept
  } else if (view === 'operator') {
    // 每种认知步类型取首次出现（8 步 Pipeline 骨架），过滤掉每候选的重复 PolicyCheck/Action
    const seen = new Set<string>()
    for (const step of COGNITIVE_STEPS) {
      const e = events.find((ev) => ev.eventType === step && !seen.has(ev.id))
      if (e) {
        seen.add(e.id)
        tlEvents.push(toTimelineEvent(e, false))
      }
    }
  } else {
    tlEvents = events.map((e) => toTimelineEvent(e, true))
  }

  return { runId, agentId, status, progress, humanApproval, events: tlEvents, decision }
}
