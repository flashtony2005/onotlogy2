import { useMemo } from 'react'
import type { AutomationJobDto } from '../types'

interface AutomationTrendPanelProps {
  jobs: AutomationJobDto[]
}

type JsonRecord = Record<string, unknown>

export interface DailyQualityPoint {
  date: string
  total: number
  succeeded: number
  failed: number
  retries: number
  reviewsSucceeded: number
  reviewsFailed: number
  successRate: number
  failureRate: number
}

export interface DataVersionPoint {
  version: string
  timestamp: number
  jobType: string
  strategyId: string | null
}

export interface StrategyBacktestPoint {
  timestamp: number
  date: string
  inputVersion: string | null
  redMean: number | null
  blueMean: number | null
  redBaseline: number | null
  blueBaseline: number | null
  redDelta: number | null
  blueDelta: number | null
  trainingWindow: number | null
  targetCount: number | null
}

export interface StrategyBacktestSeries {
  key: string
  strategyId: string
  strategyFamily: string
  strategyVersion: string
  points: StrategyBacktestPoint[]
}

export interface AutomationTrendModel {
  sampleSize: number
  completedJobs: number
  succeededJobs: number
  failedJobs: number
  totalRetries: number
  reviewSucceeded: number
  reviewFailed: number
  successRate: number
  failureRate: number
  daily: DailyQualityPoint[]
  dataVersions: DataVersionPoint[]
  strategies: StrategyBacktestSeries[]
}

function recordValue(value: unknown): JsonRecord | null {
  return value && typeof value === 'object' && !Array.isArray(value) ? value as JsonRecord : null
}

function stringValue(value: unknown): string | null {
  return typeof value === 'string' && value.trim().length > 0 ? value : null
}

function numberValue(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null
}

function timestampMillis(value?: string | null): number | null {
  if (!value) return null
  const numeric = Number(value)
  if (Number.isFinite(numeric)) {
    if (numeric >= 1e17) return numeric / 1_000_000
    if (numeric >= 1e14) return numeric / 1_000
    if (numeric >= 1e12) return numeric
    if (numeric >= 1e9) return numeric * 1_000
  }
  const parsed = Date.parse(value)
  return Number.isNaN(parsed) ? null : parsed
}

function jobTimestamp(job: AutomationJobDto): number | null {
  return timestampMillis(job.finished_at)
    ?? timestampMillis(job.updated_at)
    ?? timestampMillis(job.created_at)
}

function inputVersion(job: AutomationJobDto): string | null {
  return stringValue(job.payload.input_version) ?? stringValue(job.payload.fingerprint)
}

function unwrapResult(job: AutomationJobDto): JsonRecord | null {
  const result = recordValue(job.result)
  return recordValue(result?.result) ?? result
}

function metricDelta(value: number | null, baseline: number | null): number | null {
  return value === null || baseline === null ? null : value - baseline
}

export function buildAutomationTrendModel(jobs: AutomationJobDto[]): AutomationTrendModel {
  const datedJobs = jobs
    .map((job) => ({ job, timestamp: jobTimestamp(job) }))
    .filter((entry): entry is { job: AutomationJobDto; timestamp: number } => entry.timestamp !== null)
    .sort((left, right) => left.timestamp - right.timestamp)

  const dailyMap = new Map<string, DailyQualityPoint>()
  let succeededJobs = 0
  let failedJobs = 0
  let totalRetries = 0
  let reviewSucceeded = 0
  let reviewFailed = 0

  for (const { job, timestamp } of datedJobs) {
    const date = new Date(timestamp).toISOString().slice(0, 10)
    const point = dailyMap.get(date) ?? {
      date,
      total: 0,
      succeeded: 0,
      failed: 0,
      retries: 0,
      reviewsSucceeded: 0,
      reviewsFailed: 0,
      successRate: 0,
      failureRate: 0,
    }
    const retries = Math.max(job.attempt - 1, 0)
    point.total += 1
    point.retries += retries
    totalRetries += retries

    if (job.status === 'succeeded') {
      point.succeeded += 1
      succeededJobs += 1
      if (job.job_type === 'review') {
        point.reviewsSucceeded += 1
        reviewSucceeded += 1
      }
    } else if (job.status === 'failed') {
      point.failed += 1
      failedJobs += 1
      if (job.job_type === 'review') {
        point.reviewsFailed += 1
        reviewFailed += 1
      }
    }
    dailyMap.set(date, point)
  }

  const daily = Array.from(dailyMap.values()).map((point) => {
    const completed = point.succeeded + point.failed
    return {
      ...point,
      successRate: completed === 0 ? 0 : point.succeeded / completed,
      failureRate: completed === 0 ? 0 : point.failed / completed,
    }
  }).slice(-14)

  const versions = new Map<string, DataVersionPoint>()
  for (const { job, timestamp } of datedJobs) {
    const version = inputVersion(job)
    if (!version || versions.has(version)) continue
    versions.set(version, {
      version,
      timestamp,
      jobType: job.job_type,
      strategyId: stringValue(job.payload.strategy_id),
    })
  }

  const strategyMap = new Map<string, StrategyBacktestSeries>()
  for (const { job, timestamp } of datedJobs) {
    if (job.job_type !== 'backtest' || job.status !== 'succeeded') continue
    const result = unwrapResult(job)
    if (!result) continue
    const strategyId = stringValue(job.payload.strategy_id) ?? '未标注策略'
    const strategyFamily = stringValue(job.payload.strategy_family) ?? 'unknown'
    const strategyVersion = stringValue(job.payload.strategy_version) ?? '未标注版本'
    const key = `${strategyFamily}::${strategyId}::${strategyVersion}`
    const redMean = numberValue(result.mean_red_hits)
    const blueMean = numberValue(result.mean_blue_hits)
    const redBaseline = numberValue(result.random_baseline_red_hits)
    const blueBaseline = numberValue(result.random_baseline_blue_hits)
    const series = strategyMap.get(key) ?? {
      key,
      strategyId,
      strategyFamily,
      strategyVersion,
      points: [],
    }
    series.points.push({
      timestamp,
      date: new Date(timestamp).toISOString().slice(0, 10),
      inputVersion: inputVersion(job),
      redMean,
      blueMean,
      redBaseline,
      blueBaseline,
      redDelta: metricDelta(redMean, redBaseline),
      blueDelta: metricDelta(blueMean, blueBaseline),
      trainingWindow: numberValue(result.train_window),
      targetCount: numberValue(result.target_count),
    })
    strategyMap.set(key, series)
  }

  const completedJobs = succeededJobs + failedJobs
  return {
    sampleSize: jobs.length,
    completedJobs,
    succeededJobs,
    failedJobs,
    totalRetries,
    reviewSucceeded,
    reviewFailed,
    successRate: completedJobs === 0 ? 0 : succeededJobs / completedJobs,
    failureRate: completedJobs === 0 ? 0 : failedJobs / completedJobs,
    daily,
    dataVersions: Array.from(versions.values()).sort((left, right) => right.timestamp - left.timestamp).slice(0, 8),
    strategies: Array.from(strategyMap.values()).sort((left, right) => {
      const leftTime = left.points.at(-1)?.timestamp ?? 0
      const rightTime = right.points.at(-1)?.timestamp ?? 0
      return rightTime - leftTime
    }),
  }
}

function percent(value: number): string {
  return `${(value * 100).toFixed(1)}%`
}

function decimal(value: number | null): string {
  return value === null ? '—' : value.toFixed(2)
}

function delta(value: number | null): string {
  if (value === null) return '—'
  return `${value >= 0 ? '+' : ''}${value.toFixed(2)}`
}

function shortDate(value: string): string {
  return value.slice(5).replace('-', '/')
}

function operationLabel(value: string): string {
  switch (value) {
    case 'sync': return '同步'
    case 'import': return '导入'
    case 'analyze': return '分析'
    case 'backtest': return '回测'
    case 'review': return 'Pi 复核'
    default: return value
  }
}

export function AutomationTrendPanel({ jobs }: AutomationTrendPanelProps) {
  const model = useMemo(() => buildAutomationTrendModel(jobs), [jobs])
  const maxRetries = Math.max(...model.daily.map((point) => point.retries), 1)

  return (
    <section className="automation-trend-panel" aria-label="自动运行质量趋势">
      <div className="automation-trend-head">
        <div>
          <span className="domain-label">自动运行质量趋势</span>
          <p>基于当前加载的最近 {model.sampleSize} 条任务；成功率只统计成功与失败终态，排除排队、执行中和已取消任务。</p>
        </div>
        <span className="automation-trend-window">最多 14 个活跃日</span>
      </div>

      <div className="automation-trend-summary">
        <div><b>{percent(model.successRate)}</b><span>成功率</span><small>{model.succeededJobs}/{model.completedJobs} 个终态任务</small></div>
        <div><b>{percent(model.failureRate)}</b><span>失败率</span><small>{model.failedJobs} 个失败任务</small></div>
        <div><b>{model.totalRetries}</b><span>累计重试次数</span><small>attempt - 1</small></div>
        <div><b>{model.reviewSucceeded}/{model.reviewFailed}</b><span>Pi 复核成功/失败</span><small>仅统计 review 任务</small></div>
      </div>

      <div className="automation-trend-grid">
        <article className="automation-trend-card quality">
          <div className="automation-trend-card-head"><strong>每日成功 / 失败趋势</strong><span>最近任务窗口</span></div>
          {model.daily.length === 0 ? <div className="automation-trend-empty">暂无可用于趋势计算的任务。</div> : (
            <div className="automation-daily-list">
              {model.daily.map((point) => (
                <div className="automation-daily-row" key={point.date}>
                  <time dateTime={point.date}>{shortDate(point.date)}</time>
                  <div className="automation-rate-track" title={`成功 ${point.succeeded}，失败 ${point.failed}`}>
                    <span className="succeeded" style={{ width: `${point.successRate * 100}%` }} />
                    <span className="failed" style={{ width: `${point.failureRate * 100}%` }} />
                  </div>
                  <span className="automation-rate-value">{percent(point.successRate)} / {percent(point.failureRate)}</span>
                  <div className="automation-retry-track" title={`重试 ${point.retries} 次`}>
                    <span style={{ width: `${(point.retries / maxRetries) * 100}%` }} />
                  </div>
                  <b>{point.retries}</b>
                </div>
              ))}
              <div className="automation-daily-legend"><span className="succeeded">成功率</span><span className="failed">失败率</span><span className="retry">重试次数</span></div>
            </div>
          )}
        </article>

        <article className="automation-trend-card reviews">
          <div className="automation-trend-card-head"><strong>Pi Agent 复核趋势</strong><span>按日独立统计</span></div>
          {model.daily.every((point) => point.reviewsSucceeded === 0 && point.reviewsFailed === 0) ? (
            <div className="automation-trend-empty">暂无 Pi Agent 复核终态记录。</div>
          ) : (
            <div className="automation-review-trend">
              {model.daily.map((point) => {
                const total = point.reviewsSucceeded + point.reviewsFailed
                if (total === 0) return null
                return (
                  <div key={point.date}>
                    <time dateTime={point.date}>{shortDate(point.date)}</time>
                    <span className="review-success" style={{ height: `${Math.max(point.reviewsSucceeded * 12, point.reviewsSucceeded ? 6 : 0)}px` }} title={`成功 ${point.reviewsSucceeded}`} />
                    <span className="review-failed" style={{ height: `${Math.max(point.reviewsFailed * 12, point.reviewsFailed ? 6 : 0)}px` }} title={`失败 ${point.reviewsFailed}`} />
                    <b>{point.reviewsSucceeded}/{point.reviewsFailed}</b>
                  </div>
                )
              })}
            </div>
          )}
        </article>

        <article className="automation-trend-card versions">
          <div className="automation-trend-card-head"><strong>数据版本变化</strong><span>同一版本仅记录首次出现</span></div>
          {model.dataVersions.length === 0 ? <div className="automation-trend-empty">暂无 input_version 或 fingerprint。</div> : (
            <div className="automation-version-list">
              {model.dataVersions.map((point) => (
                <div key={point.version}>
                  <span className="automation-version-dot" />
                  <div><code>{point.version.slice(0, 18)}</code><small>{new Date(point.timestamp).toLocaleString()} · {operationLabel(point.jobType)}{point.strategyId ? ` · ${point.strategyId}` : ''}</small></div>
                </div>
              ))}
            </div>
          )}
        </article>
      </div>

      <div className="automation-strategy-section">
        <div className="automation-trend-card-head">
          <strong>分策略回测表现</strong>
          <span>策略家族、策略 ID、版本分别隔离；不生成跨策略总分</span>
        </div>
        {model.strategies.length === 0 ? <div className="automation-trend-empty">暂无成功的回测任务。</div> : (
          <div className="automation-strategy-grid">
            {model.strategies.map((series) => {
              const latest = series.points.at(-1)
              return (
                <article className="automation-strategy-card" key={series.key}>
                  <div className="automation-strategy-head">
                    <strong>{series.strategyId}</strong>
                    <span>{series.strategyFamily} · {series.strategyVersion}</span>
                  </div>
                  <div className="automation-strategy-metrics">
                    <div><b>{decimal(latest?.redMean ?? null)}</b><span>前区均值</span><small>基线 {decimal(latest?.redBaseline ?? null)}</small></div>
                    <div><b className={(latest?.redDelta ?? -1) >= 0 ? 'positive' : 'negative'}>{delta(latest?.redDelta ?? null)}</b><span>前区差值</span><small>独立指标</small></div>
                    <div><b>{decimal(latest?.blueMean ?? null)}</b><span>后区均值</span><small>基线 {decimal(latest?.blueBaseline ?? null)}</small></div>
                    <div><b className={(latest?.blueDelta ?? -1) >= 0 ? 'positive' : 'negative'}>{delta(latest?.blueDelta ?? null)}</b><span>后区差值</span><small>独立指标</small></div>
                  </div>
                  <div className="automation-strategy-points">
                    {series.points.slice(-6).map((point) => (
                      <div key={`${point.timestamp}:${point.inputVersion ?? ''}`}>
                        <time dateTime={point.date}>{shortDate(point.date)}</time>
                        <span>前 {delta(point.redDelta)}</span>
                        <span>后 {delta(point.blueDelta)}</span>
                        <code>{point.inputVersion?.slice(0, 10) ?? '无版本'}</code>
                      </div>
                    ))}
                  </div>
                  <div className="automation-strategy-foot">
                    <span>{series.points.length} 次回测</span>
                    <span>窗口 {latest?.trainingWindow ?? '—'}</span>
                    <span>测试 {latest?.targetCount ?? '—'} 期</span>
                  </div>
                </article>
              )
            })}
          </div>
        )}
        <p className="automation-trend-disclaimer">趋势只描述历史任务质量与历史样本回测，不代表未来中奖概率，也不提供投注建议。</p>
      </div>
    </section>
  )
}
