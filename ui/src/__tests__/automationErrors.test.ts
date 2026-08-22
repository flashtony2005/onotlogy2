import { describe, expect, it } from 'vitest'
import { buildAutomationErrorGroups } from '../components/AutomationPanel'
import type { AutomationJobDto } from '../types'

function job(overrides: Partial<AutomationJobDto> & Pick<AutomationJobDto, 'id' | 'job_type' | 'status'>): AutomationJobDto {
  return {
    pack_id: 'lottery-analysis',
    dedupe_key: overrides.id,
    payload: {},
    attempt: 1,
    max_attempts: 3,
    available_at: '2026-08-20T00:00:00Z',
    created_at: '2026-08-20T00:00:00Z',
    updated_at: '2026-08-20T00:00:00Z',
    ...overrides,
  }
}

describe('buildAutomationErrorGroups', () => {
  it('同阶段在失败后成功时标记为已恢复', () => {
    const groups = buildAutomationErrorGroups([
      job({ id: 'failed', job_type: 'sync', status: 'failed', last_error: 'duplicate_issue', updated_at: '2026-08-20T01:00:00Z' }),
      job({ id: 'succeeded', job_type: 'sync', status: 'succeeded', updated_at: '2026-08-20T02:00:00Z' }),
    ])

    expect(groups).toEqual([expect.objectContaining({
      message: 'duplicate_issue',
      count: 1,
      recovered: true,
      recoveredAt: '2026-08-20T02:00:00Z',
    })])
  })

  it('成功早于失败时仍标记为活动故障', () => {
    const groups = buildAutomationErrorGroups([
      job({ id: 'succeeded', job_type: 'sync', status: 'succeeded', updated_at: '2026-08-20T01:00:00Z' }),
      job({ id: 'failed', job_type: 'sync', status: 'failed', last_error: 'file_missing', updated_at: '2026-08-20T02:00:00Z' }),
    ])

    expect(groups[0]).toMatchObject({ recovered: false, recoveredAt: null })
  })

  it('有策略的失败只能由同策略同阶段成功恢复', () => {
    const failed = job({
      id: 'failed',
      job_type: 'backtest',
      status: 'failed',
      last_error: 'backtest_failed',
      payload: { strategy_id: 'statistics' },
      updated_at: '2026-08-20T01:00:00Z',
    })
    const mismatch = job({
      id: 'mismatch',
      job_type: 'backtest',
      status: 'succeeded',
      payload: { strategy_id: 'heuristic' },
      updated_at: '2026-08-20T02:00:00Z',
    })

    expect(buildAutomationErrorGroups([failed, mismatch])[0].recovered).toBe(false)

    const matching = job({
      id: 'matching',
      job_type: 'backtest',
      status: 'succeeded',
      payload: { strategy_id: 'statistics' },
      updated_at: '2026-08-20T03:00:00Z',
    })
    expect(buildAutomationErrorGroups([failed, mismatch, matching])[0]).toMatchObject({
      recovered: true,
      recoveredAt: '2026-08-20T03:00:00Z',
    })
  })

  it('缺少策略的旧 review 可由后续成功 review 恢复', () => {
    const groups = buildAutomationErrorGroups([
      job({ id: 'failed', job_type: 'review', status: 'failed', last_error: 'strategy_id_missing', updated_at: '2026-08-20T01:00:00Z' }),
      job({ id: 'succeeded', job_type: 'review', status: 'succeeded', payload: { strategy_id: 'statistics' }, updated_at: '2026-08-20T02:00:00Z' }),
    ])

    expect(groups[0].recovered).toBe(true)
  })

  it('聚合相同错误并以最后一次失败判断恢复时间', () => {
    const groups = buildAutomationErrorGroups([
      job({ id: 'failed-1', job_type: 'sync', status: 'failed', last_error: '  duplicate   issue  ', updated_at: '2026-08-20T01:00:00Z' }),
      job({ id: 'failed-2', job_type: 'sync', status: 'failed', last_error: 'duplicate issue', updated_at: '2026-08-20T02:00:00Z' }),
      job({ id: 'succeeded', job_type: 'sync', status: 'succeeded', updated_at: '2026-08-20T03:00:00Z' }),
    ])

    expect(groups[0]).toMatchObject({
      message: 'duplicate issue',
      count: 2,
      latestFailedAt: '2026-08-20T02:00:00Z',
      recovered: true,
      recoveredAt: '2026-08-20T03:00:00Z',
    })
  })

  it('未恢复错误优先于已恢复历史错误展示', () => {
    const groups = buildAutomationErrorGroups([
      job({ id: 'old-failed', job_type: 'sync', status: 'failed', last_error: 'recovered_error', updated_at: '2026-08-20T01:00:00Z' }),
      job({ id: 'old-success', job_type: 'sync', status: 'succeeded', updated_at: '2026-08-20T02:00:00Z' }),
      job({ id: 'active-failed', job_type: 'review', status: 'failed', last_error: 'active_error', updated_at: '2026-08-20T03:00:00Z' }),
    ])

    expect(groups.map((group) => [group.message, group.recovered])).toEqual([
      ['active_error', false],
      ['recovered_error', true],
    ])
  })
})
