import { describe, expect, it } from 'vitest'
import { buildAutomationTrendModel } from '../components/AutomationTrendPanel'
import type { AutomationJobDto } from '../types'

function job(overrides: Partial<AutomationJobDto> & Pick<AutomationJobDto, 'id' | 'job_type' | 'status'>): AutomationJobDto {
  return {
    pack_id: 'lottery',
    dedupe_key: overrides.id,
    payload: {},
    attempt: 1,
    max_attempts: 3,
    available_at: '2026-08-18T00:00:00Z',
    created_at: '2026-08-18T00:00:00Z',
    updated_at: '2026-08-18T00:00:00Z',
    ...overrides,
  }
}

describe('buildAutomationTrendModel', () => {
  it('按日计算成功率、失败率、重试次数和 Pi 复核结果', () => {
    const model = buildAutomationTrendModel([
      job({ id: 'a', job_type: 'analyze', status: 'succeeded', attempt: 1 }),
      job({ id: 'b', job_type: 'backtest', status: 'failed', attempt: 3 }),
      job({ id: 'c', job_type: 'review', status: 'succeeded', attempt: 2 }),
      job({ id: 'd', job_type: 'review', status: 'failed', attempt: 1 }),
      job({ id: 'e', job_type: 'sync', status: 'queued', attempt: 0 }),
    ])

    expect(model.successRate).toBe(0.5)
    expect(model.failureRate).toBe(0.5)
    expect(model.totalRetries).toBe(3)
    expect(model.reviewSucceeded).toBe(1)
    expect(model.reviewFailed).toBe(1)
    expect(model.daily).toHaveLength(1)
    expect(model.daily[0]).toMatchObject({ succeeded: 2, failed: 2, retries: 3, successRate: 0.5, failureRate: 0.5 })
  })

  it('数据版本去重并保留首次出现的任务信息', () => {
    const model = buildAutomationTrendModel([
      job({ id: 'first', job_type: 'sync', status: 'succeeded', payload: { input_version: 'v1' }, updated_at: '2026-08-18T01:00:00Z' }),
      job({ id: 'repeat', job_type: 'analyze', status: 'succeeded', payload: { input_version: 'v1', strategy_id: 'frequency' }, updated_at: '2026-08-18T02:00:00Z' }),
      job({ id: 'second', job_type: 'sync', status: 'succeeded', payload: { fingerprint: 'v2' }, updated_at: '2026-08-19T01:00:00Z' }),
    ])

    expect(model.dataVersions.map((point) => point.version)).toEqual(['v2', 'v1'])
    expect(model.dataVersions[1].jobType).toBe('sync')
    expect(model.dataVersions[1].strategyId).toBeNull()
  })

  it('按策略家族、ID 和版本隔离回测，不产生跨策略评分', () => {
    const commonResult = {
      result: {
        mean_red_hits: 0.8,
        random_baseline_red_hits: 0.7,
        mean_blue_hits: 0.2,
        random_baseline_blue_hits: 0.3,
        train_window: 50,
        target_count: 30,
      },
    }
    const model = buildAutomationTrendModel([
      job({ id: 's1', job_type: 'backtest', status: 'succeeded', payload: { strategy_family: 'statistics', strategy_id: 'frequency', strategy_version: '1.0.0', input_version: 'v1' }, result: commonResult }),
      job({ id: 's2', job_type: 'backtest', status: 'succeeded', payload: { strategy_family: 'statistics', strategy_id: 'frequency', strategy_version: '2.0.0', input_version: 'v2' }, result: commonResult, updated_at: '2026-08-19T00:00:00Z' }),
      job({ id: 'h1', job_type: 'backtest', status: 'succeeded', payload: { strategy_family: 'heuristic', strategy_id: 'frequency', strategy_version: '1.0.0', input_version: 'v3' }, result: commonResult, updated_at: '2026-08-20T00:00:00Z' }),
      job({ id: 'failed', job_type: 'backtest', status: 'failed', payload: { strategy_family: 'metaphysics', strategy_id: 'experimental', strategy_version: '1.0.0' }, result: commonResult }),
    ])

    expect(model.strategies).toHaveLength(3)
    expect(model.strategies.map((series) => series.key)).toEqual(expect.arrayContaining([
      'statistics::frequency::1.0.0',
      'statistics::frequency::2.0.0',
      'heuristic::frequency::1.0.0',
    ]))
    expect(model.strategies.some((series) => series.strategyFamily === 'metaphysics')).toBe(false)
    const point = model.strategies[0].points[0]
    expect(point.redDelta).toBeCloseTo(0.1)
    expect(point.blueDelta).toBeCloseTo(-0.1)
  })
})
