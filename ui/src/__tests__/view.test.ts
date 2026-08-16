// view.ts 纯函数单测：状态推导（失败/审批/完成）、三层视图裁剪、时间排序。
// 与后端 trace.rs 同源逻辑的前端防线——防成对 bug 复发。

import { describe, expect, it } from 'vitest'
import { buildTimeline } from '../view'
import type { TraceEventDto } from '../types'

function ev(over: Partial<TraceEventDto> & { eventType: string }): TraceEventDto {
  return {
    id: `tr_${Math.random().toString(36).slice(2)}`,
    kind: 'cognitive',
    timestamp: '1',
    ...over,
  }
}

describe('buildTimeline 状态推导', () => {
  it('MemoryUpdate → Completed', () => {
    const tl = buildTimeline(
      [ev({ eventType: 'MemoryUpdate' })],
      'r1',
      'a',
      'developer',
    )
    expect(tl.status).toBe('Completed')
    expect(tl.humanApproval).toBe('None')
  })

  it('ActionFailed → Failed（优先于 MemoryUpdate）', () => {
    const tl = buildTimeline(
      [
        ev({ eventType: 'Action' }),
        ev({ eventType: 'ActionFailed' }),
        ev({ eventType: 'MemoryUpdate' }),
      ],
      'r1',
      'a',
      'developer',
    )
    expect(tl.status).toBe('Failed')
  })

  it('PolicyCheck Deny → Failed + Required', () => {
    const tl = buildTimeline(
      [ev({ eventType: 'PolicyCheck', output: 'Deny' })],
      'r1',
      'a',
      'developer',
    )
    expect(tl.status).toBe('Failed')
    expect(tl.humanApproval).toBe('Required')
  })

  it('PendingApproval 挂起中 → Required（即使尚无 Deny）', () => {
    const tl = buildTimeline(
      [ev({ eventType: 'PendingApproval', input: { reqId: 'apr_1' } })],
      'r1',
      'a',
      'developer',
    )
    expect(tl.humanApproval).toBe('Required')
    expect(tl.status).toBe('Running')
  })

  it('无任何事件 → Running + None', () => {
    const tl = buildTimeline([], 'r1', 'a', 'developer')
    expect(tl.status).toBe('Running')
    expect(tl.humanApproval).toBe('None')
  })
})

describe('buildTimeline 三层视图', () => {
  const events = [
    ev({ eventType: 'Goal', input: { goal: 'reduce churn' }, reasoning: 'goal 推理' }),
    ev({ eventType: 'Reasoning', reasoning: '内部推理', confidence: 0.4 }),
    ev({ eventType: 'Action', output: 'ok', reasoning: '动作推理' }),
  ]

  it('Executive：仅 Goal/Action，剥细节，risk = 1 - max(confidence)', () => {
    const tl = buildTimeline(events, 'r1', 'a', 'executive')
    expect(tl.events.map((e) => e.eventType)).toEqual(['Goal', 'Action'])
    expect(tl.events.every((e) => e.reasoning === undefined)).toBe(true)
    // Reasoning 最高置信 0.4 → risk = 0.60（低置信 = 高风险）
    expect(tl.decision?.risk).toBe('0.60')
    expect(tl.decision?.mission).toBe('reduce churn')
  })

  it('Operator：8 步骨架去重', () => {
    const tl = buildTimeline(events, 'r1', 'a', 'operator')
    expect(tl.events.every((e) => e.reasoning === undefined)).toBe(true)
    expect(tl.events.some((e) => e.eventType === 'Reasoning')).toBe(true)
  })

  it('Developer：全量保留细节', () => {
    const tl = buildTimeline(events, 'r1', 'a', 'developer')
    expect(tl.events.length).toBe(3)
    expect(tl.events.some((e) => e.reasoning !== undefined)).toBe(true)
  })

})

describe('buildTimeline 事件排序', () => {
  it('乱序输入按 timestamp 稳定排序后再渲染', () => {
    const events = [
      ev({ eventType: 'Action', timestamp: '5' }),
      ev({ eventType: 'Goal', timestamp: '1' }),
      ev({ eventType: 'Reasoning', timestamp: '3' }),
    ]
    const tl = buildTimeline(events, 'r1', 'a', 'developer')
    expect(tl.events.map((e) => e.timestamp)).toEqual(['1', '3', '5'])
  })
})
