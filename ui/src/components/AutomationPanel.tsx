import { useEffect, useMemo, useState } from 'react'
import { LotteryResultPanel } from './LotteryResultPanel'
import { AutomationTrendPanel } from './AutomationTrendPanel'
import {
  cancelAutomationJob,
  getAutomationJobs,
  getDomainPackAutomationStatus,
  getTrace,
  runDomainPack,
  runDomainPackOperation,
  scanDomainPack,
} from '../api'
import type {
  AutomationJobDto,
  AutomationPackStatusDto,
  AutomationScanHistoryDto,
  AutomationStatusDto,
  DomainPackOperation,
  TraceEventDto,
} from '../types'

interface AutomationPanelProps {
  packId: string
  packName: string
  currentRole: string
  onAuthError: (error: unknown) => boolean
}

type ScanFilter = 'all' | 'failed' | 'succeeded'
type JobStatusFilter = 'all' | 'queued' | 'running' | 'succeeded' | 'failed' | 'cancelled'
type JobTypeFilter = 'all' | DomainPackOperation
type JobTimeFilter = 'all' | '24h' | '7d' | '30d'

const OPERATIONS: Array<{ id: DomainPackOperation; label: string }> = [
  { id: 'sync', label: '同步 / 导入' },
  { id: 'analyze', label: '统计分析' },
  { id: 'backtest', label: '回测' },
  { id: 'review', label: 'Pi 复核' },
]

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

function statusLabel(status: string): string {
  switch (status) {
    case 'queued': return '排队中'
    case 'running': return '执行中'
    case 'succeeded': return '成功'
    case 'failed': return '失败'
    case 'cancelled': return '已取消'
    default: return status
  }
}

function scanStateLabel(state: string): string {
  switch (state) {
    case 'never': return '尚未扫描'
    case 'healthy': return '扫描正常'
    case 'failed': return '扫描失败'
    case 'recovered': return '已恢复'
    default: return state
  }
}

function historyStateLabel(state: string): string {
  return state === 'failed' ? '失败' : '成功'
}

function formatTime(value?: string | null): string {
  if (!value) return '—'
  const timestamp = Number(value)
  if (!Number.isNaN(timestamp) && timestamp > 1_000_000_000_000) {
    return new Date(timestamp / 1000).toLocaleString()
  }
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString()
}

function stringValue(value: unknown): string | null {
  return typeof value === 'string' && value.length > 0 ? value : null
}

function numberValue(value: unknown): number | null {
  return typeof value === 'number' ? value : null
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

function jobInputVersion(job: AutomationJobDto): string | null {
  return stringValue(job.payload.input_version) ?? stringValue(job.payload.fingerprint)
}

function normalizedJobError(job: AutomationJobDto): string | null {
  if (!job.last_error) return null
  const normalized = job.last_error.replace(/\s+/g, ' ').trim()
  return normalized.length > 180 ? normalized.slice(0, 177) + '…' : normalized
}

export interface AutomationErrorGroup {
  message: string
  count: number
  latestFailedAt: string
  recovered: boolean
  recoveredAt: string | null
}

function jobActivityTime(job: AutomationJobDto): { value: string; millis: number } | null {
  for (const value of [job.updated_at, job.finished_at, job.created_at]) {
    const millis = timestampMillis(value)
    if (value && millis !== null) return { value, millis }
  }
  return null
}

function recoveryFor(failedJob: AutomationJobDto, jobs: AutomationJobDto[]): { value: string; millis: number } | null {
  const failedAt = jobActivityTime(failedJob)
  if (!failedAt) return null
  const failedStrategy = stringValue(failedJob.payload.strategy_id)
  let recovery: { value: string; millis: number } | null = null

  for (const candidate of jobs) {
    if (candidate.status !== 'succeeded') continue
    if (candidate.pack_id !== failedJob.pack_id || candidate.job_type !== failedJob.job_type) continue
    if (failedStrategy !== null && stringValue(candidate.payload.strategy_id) !== failedStrategy) continue
    const candidateAt = jobActivityTime(candidate)
    if (!candidateAt || candidateAt.millis <= failedAt.millis) continue
    if (!recovery || candidateAt.millis < recovery.millis) recovery = candidateAt
  }
  return recovery
}

export function buildAutomationErrorGroups(jobs: AutomationJobDto[]): AutomationErrorGroup[] {
  const grouped = new Map<string, AutomationJobDto[]>()
  jobs.forEach((job) => {
    if (job.status !== 'failed') return
    const message = normalizedJobError(job)
    if (!message) return
    grouped.set(message, [...(grouped.get(message) ?? []), job])
  })

  return Array.from(grouped.entries()).map(([message, failedJobs]) => {
    const latestFailure = failedJobs
      .map((job) => jobActivityTime(job))
      .filter((value): value is { value: string; millis: number } => value !== null)
      .sort((left, right) => right.millis - left.millis)[0]
    const recoveries = failedJobs.map((job) => recoveryFor(job, jobs))
    const recovered = recoveries.every((recovery) => recovery !== null)
    const recoveredAt = recovered
      ? recoveries
          .filter((value): value is { value: string; millis: number } => value !== null)
          .sort((left, right) => right.millis - left.millis)[0]?.value ?? null
      : null

    return {
      message,
      count: failedJobs.length,
      latestFailedAt: latestFailure?.value ?? failedJobs[0].updated_at,
      recovered,
      recoveredAt,
    }
  }).sort((left, right) => {
    if (left.recovered !== right.recovered) return left.recovered ? 1 : -1
    const latest = (timestampMillis(right.latestFailedAt) ?? 0) - (timestampMillis(left.latestFailedAt) ?? 0)
    return latest || right.count - left.count || left.message.localeCompare(right.message)
  }).slice(0, 5)
}

function jobSummary(job: AutomationJobDto): string {
  const payload = job.payload
  const result = job.result
  const nestedResult = result?.result
  const output = nestedResult && typeof nestedResult === 'object' && !Array.isArray(nestedResult)
    ? nestedResult as Record<string, unknown>
    : result
  const inputVersion = stringValue(payload.input_version) ?? stringValue(payload.fingerprint)
  const rows = numberValue(payload.source_rows)
  const newIssues = numberValue(payload.new_issue_count)
  const sampleSize = numberValue(output?.sample_size)
  const targetCount = numberValue(output?.target_count)
  const parts = [
    inputVersion ? '数据 ' + inputVersion.slice(0, 12) : null,
    rows !== null ? String(rows) + ' 行' : null,
    newIssues !== null ? '新增 ' + String(newIssues) + ' 期' : null,
    sampleSize !== null ? '样本 ' + String(sampleSize) : null,
    targetCount !== null ? '回测 ' + String(targetCount) + ' 期' : null,
  ]
  return parts.filter((part): part is string => part !== null).join(' · ') || '暂无输出摘要'
}

function reviewSummary(job: AutomationJobDto): string | null {
  if (job.job_type !== 'review' || !job.result) return null
  const capability = stringValue(job.result.capability)
  const confidence = numberValue(job.result.confidence)
  const reasoning = stringValue(job.result.reasoning)
  const strategy = stringValue(job.payload.strategy_id)
  const parts = [
    capability ? `能力 ${capability}` : '未追加动作',
    confidence !== null ? `置信度 ${(confidence * 100).toFixed(0)}%` : null,
    strategy ? `策略 ${strategy}` : null,
    reasoning,
  ]
  return parts.filter((part): part is string => part !== null).join(' · ')
}

function isCancellable(job: AutomationJobDto): boolean {
  return job.status === 'queued' || job.status === 'running'
}

function historySummary(entry: AutomationScanHistoryDto): string {
  if (entry.status === 'failed') {
    return `${entry.code || 'source_unavailable'}：${entry.message || '数据源扫描失败'}`
  }
  return entry.fingerprint ? `数据版本 ${entry.fingerprint.slice(0, 16)}` : '发现数据源变化'
}

export function AutomationPanel({ packId, packName, currentRole, onAuthError }: AutomationPanelProps) {
  const [status, setStatus] = useState<AutomationStatusDto | null>(null)
  const [packStatus, setPackStatus] = useState<AutomationPackStatusDto | null>(null)
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [scanFilter, setScanFilter] = useState<ScanFilter>('all')
  const [trace, setTrace] = useState<TraceEventDto | null>(null)
  const [traceBusy, setTraceBusy] = useState(false)
  const [traceError, setTraceError] = useState<string | null>(null)
  const [jobStatusFilter, setJobStatusFilter] = useState<JobStatusFilter>('all')
  const [jobTypeFilter, setJobTypeFilter] = useState<JobTypeFilter>('all')
  const [strategyFilter, setStrategyFilter] = useState('all')
  const [dataVersionFilter, setDataVersionFilter] = useState('all')
  const [jobTimeFilter, setJobTimeFilter] = useState<JobTimeFilter>('all')
  const canTrigger = ['operator', 'developer'].includes(currentRole.toLowerCase())

  const jobs = useMemo(
    () => (status?.jobs ?? []).filter((job) => job.pack_id === packId),
    [packId, status],
  )
  const active = jobs.some((job) => job.status === 'queued' || job.status === 'running')
  const history = useMemo(() => {
    const entries = packStatus?.scan_history ?? []
    return scanFilter === 'all' ? entries : entries.filter((entry) => entry.status === scanFilter)
  }, [packStatus?.scan_history, scanFilter])

  const strategyOptions = useMemo(
    () => Array.from(new Set(jobs.map((job) => stringValue(job.payload.strategy_id)).filter((value): value is string => value !== null))).sort(),
    [jobs],
  )
  const dataVersionOptions = useMemo(
    () => Array.from(new Set(jobs.map(jobInputVersion).filter((value): value is string => value !== null))).sort().reverse(),
    [jobs],
  )
  const filteredJobs = useMemo(() => {
    const now = Date.now()
    const maxAge = jobTimeFilter === '24h'
      ? 24 * 60 * 60 * 1_000
      : jobTimeFilter === '7d'
        ? 7 * 24 * 60 * 60 * 1_000
        : jobTimeFilter === '30d'
          ? 30 * 24 * 60 * 60 * 1_000
          : null
    return jobs.filter((job) => {
      if (jobStatusFilter !== 'all' && job.status !== jobStatusFilter) return false
      if (jobTypeFilter !== 'all' && job.job_type !== jobTypeFilter) return false
      if (strategyFilter !== 'all' && stringValue(job.payload.strategy_id) !== strategyFilter) return false
      if (dataVersionFilter !== 'all' && jobInputVersion(job) !== dataVersionFilter) return false
      if (maxAge !== null) {
        const createdAt = timestampMillis(job.created_at)
        if (createdAt === null || now - createdAt > maxAge) return false
      }
      return true
    })
  }, [dataVersionFilter, jobStatusFilter, jobTimeFilter, jobTypeFilter, jobs, strategyFilter])
  const errorGroups = useMemo(() => buildAutomationErrorGroups(jobs), [jobs])

  const load = async (silent = false) => {
    if (!silent) setLoading(true)
    setError(null)
    try {
      const [automationStatus, nextPackStatus] = await Promise.all([
        getAutomationJobs(),
        getDomainPackAutomationStatus(packId),
      ])
      setStatus(automationStatus)
      setPackStatus(nextPackStatus)
    } catch (e) {
      if (!onAuthError(e)) setError(errorMessage(e))
    } finally {
      if (!silent) setLoading(false)
    }
  }

  useEffect(() => {
    void load()
  }, [packId])

  useEffect(() => {
    setJobStatusFilter('all')
    setJobTypeFilter('all')
    setStrategyFilter('all')
    setDataVersionFilter('all')
    setJobTimeFilter('all')
  }, [packId])

  useEffect(() => {
    if (!active && !packStatus?.runtime.watch_enabled) return
    const timer = window.setInterval(() => void load(true), 3000)
    return () => window.clearInterval(timer)
  }, [active, packStatus?.runtime.watch_enabled, packId])

  const run = async (label: string, action: () => Promise<unknown>) => {
    setBusy(label)
    setError(null)
    setNotice(null)
    try {
      await action()
      setNotice(packName + '：' + label + '任务已提交')
      await load(true)
    } catch (e) {
      try {
        await load(true)
      } catch {
        // 保留原始操作错误；状态刷新失败不应覆盖它。
      }
      if (!onAuthError(e)) setError(errorMessage(e))
    } finally {
      setBusy(null)
    }
  }

  const scan = async (label: string) => {
    setBusy(label)
    setError(null)
    setNotice(null)
    try {
      const jobs = await scanDomainPack(packId)
      setNotice(
        jobs.length > 0
          ? `${packName}：${label}完成，已提交 ${jobs.length} 个同步任务`
          : `${packName}：${label}完成，未发现新的数据版本`,
      )
      await load(true)
    } catch (e) {
      try {
        await load(true)
      } catch {
        // 保留原始扫描错误；状态刷新失败不应覆盖它。
      }
      if (!onAuthError(e)) setError(errorMessage(e))
    } finally {
      setBusy(null)
    }
  }

  const cancel = async (job: AutomationJobDto) => {
    setBusy(job.id)
    setError(null)
    setNotice(null)
    try {
      await cancelAutomationJob(job.id)
      setNotice('自动化任务已取消')
      await load(true)
    } catch (e) {
      if (!onAuthError(e)) setError(errorMessage(e))
    } finally {
      setBusy(null)
    }
  }

  const viewTrace = async (traceId: string) => {
    setTraceBusy(true)
    setTraceError(null)
    try {
      setTrace(await getTrace(traceId))
    } catch (e) {
      if (!onAuthError(e)) setTraceError(errorMessage(e))
    } finally {
      setTraceBusy(false)
    }
  }

  return (
    <section className="automation-panel">
      <div className="domain-section-head">
        <div>
          <span className="domain-label">自动运行与真实数据验证</span>
          <p>任务由后端 watcher、队列和彩票插件执行；网页端只负责触发和观察，不直接读取 CSV。</p>
        </div>
        <div className="automation-head-actions">
          <span className={'automation-engine ' + (status?.running ? 'running' : 'stopped')}>
            {status?.running ? '引擎运行中' : '引擎未运行'}
          </span>
          <button className="run secondary" onClick={() => void load()} disabled={loading}>
            {loading ? '读取中…' : '刷新任务'}
          </button>
        </div>
      </div>

      <LotteryResultPanel jobs={jobs} />
      <AutomationTrendPanel jobs={jobs} />

      {packStatus && (
        <div className="automation-source-status">
          <div className="automation-source-status-head">
            <strong>数据源监测状态</strong>
            <span className={'automation-status scan-' + packStatus.scan_state}>
              {scanStateLabel(packStatus.scan_state)}
            </span>
          </div>
          <div className="automation-source-meta">
            <span>数据源 <code>{packStatus.runtime.source_path || '未配置'}</code></span>
            <span>Watcher {packStatus.runtime.watch_enabled ? '已启用' : '未启用'}</span>
            <span>轮询 {packStatus.runtime.poll_interval_seconds}s</span>
            <span>已接受版本 <code>{packStatus.fingerprint || '—'}</code></span>
            <span>最后成功扫描 {formatTime(packStatus.last_success_at)}</span>
            <span>人工扫描 {packStatus.manual_retry_count} 次</span>
          </div>

          <div className="automation-quality" aria-label="自动运行质量">
            <strong>运行质量</strong>
            <span>总任务 {packStatus.quality.total_jobs}</span>
            <span>成功 {packStatus.quality.succeeded_jobs}</span>
            <span>失败 {packStatus.quality.failed_jobs}</span>
            <span>排队 {packStatus.quality.queued_jobs}</span>
            <span>执行中 {packStatus.quality.running_jobs}</span>
            <span>重试 {packStatus.quality.retry_jobs}</span>
            <span>成功率 {(packStatus.quality.success_rate * 100).toFixed(1)}%</span>
            <span>
              最近 Pi 复核 {packStatus.quality.last_review_status ? statusLabel(packStatus.quality.last_review_status) : '—'}
              {packStatus.quality.last_review_at ? ` · ${formatTime(packStatus.quality.last_review_at)}` : ''}
            </span>
          </div>

          {packStatus.last_scan_error && (
            <div className={'automation-scan-error ' + (packStatus.active_scan_error ? '' : 'recovered')}>
              <div><strong>{packStatus.active_scan_error ? '当前错误：' : '历史错误（已恢复）：'}</strong><code>{packStatus.last_scan_error.code}</code></div>
              <div>{packStatus.last_scan_error.message}</div>
              <div><strong>失败路径：</strong><code>{packStatus.last_scan_error.source_path || '—'}</code></div>
              <div className="muted">
                发生于 {formatTime(packStatus.last_scan_error.timestamp)} · Trace {packStatus.last_scan_error.trace_id}
              </div>
              <div className="automation-inline-actions">
                <button className="run secondary" onClick={() => void viewTrace(packStatus.last_scan_error!.trace_id)} disabled={traceBusy}>
                  {traceBusy ? '读取 Trace…' : '查看 Trace'}
                </button>
                {canTrigger && (
                  <button className="run secondary" onClick={() => void scan('修复后重新扫描')} disabled={busy !== null}>
                    {busy === '修复后重新扫描' ? '扫描中…' : '修复后重新扫描'}
                  </button>
                )}
              </div>
              {!packStatus.active_scan_error && <div className="automation-recovered-note">数据源已恢复，当前无活动扫描错误；错误记录保留用于审计。</div>}
            </div>
          )}

          {packStatus.last_scan_summary && (
            <div className="automation-scan-summary">
              <strong>最近一次扫描摘要</strong>
              <span>数据行 {packStatus.last_scan_summary.source_rows ?? '—'}</span>
              <span>新增期号 {packStatus.last_scan_summary.new_issue_count ?? '—'}</span>
              <span>重复期号 {packStatus.last_scan_summary.duplicate_issues ?? '—'}</span>
              <span>时间 {formatTime(packStatus.last_scan_summary.timestamp)}</span>
              {packStatus.last_scan_summary.trace_id && (
                <button className="text-button" onClick={() => void viewTrace(packStatus.last_scan_summary!.trace_id!)}>
                  查看审计 Trace
                </button>
              )}
            </div>
          )}

          {packStatus.last_manual_retry && (
            <div className="automation-retry-meta">
              最近人工操作：{packStatus.last_manual_retry.actor} · 第 {packStatus.last_manual_retry.retry_number} 次 ·
              {packStatus.last_manual_retry.status === 'succeeded' ? '成功' : '失败'} · {formatTime(packStatus.last_manual_retry.timestamp)}
              <button className="text-button" onClick={() => void viewTrace(packStatus.last_manual_retry!.trace_id)}>
                查看 Trace
              </button>
            </div>
          )}

          <details className="automation-history" open>
            <summary>
              <span>扫描历史（{packStatus.scan_history.length}）</span>
              <select value={scanFilter} onChange={(event) => setScanFilter(event.target.value as ScanFilter)} onClick={(event) => event.stopPropagation()}>
                <option value="all">全部状态</option>
                <option value="failed">仅失败</option>
                <option value="succeeded">仅成功</option>
              </select>
            </summary>
            {history.length === 0 ? (
              <div className="automation-empty">没有匹配的扫描记录。</div>
            ) : (
              <div className="automation-history-list">
                {history.map((entry) => (
                  <div className="automation-history-row" key={entry.trace_id}>
                    <span className={'automation-status ' + entry.status}>{historyStateLabel(entry.status)}</span>
                    <span className="automation-history-main">
                      <strong>{entry.event_type === 'automation.source_scan_failed' ? '数据源检查' : '发现数据变化'}</strong>
                      <span>{historySummary(entry)}</span>
                    </span>
                    <span className="muted">{formatTime(entry.timestamp)}</span>
                    <button className="text-button" onClick={() => void viewTrace(entry.trace_id)}>Trace</button>
                  </div>
                ))}
              </div>
            )}
          </details>
        </div>
      )}

      {trace && (
        <div className="automation-trace-viewer">
          <div className="automation-trace-head">
            <strong>审计 Trace</strong>
            <button className="text-button" onClick={() => setTrace(null)}>关闭</button>
          </div>
          <div className="automation-source-meta"><span>ID <code>{trace.id}</code></span><span>{trace.eventType}</span><span>{formatTime(trace.timestamp)}</span></div>
          <pre>{JSON.stringify(trace, null, 2)}</pre>
        </div>
      )}
      {traceError && <div className="err-msg">⚠️ Trace 读取失败：{traceError}</div>}

      {canTrigger && (
        <div className="automation-actions">
          <button className="run" onClick={() => void run('完整链路', () => runDomainPack(packId))} disabled={busy !== null}>
            {busy === '完整链路' ? '提交中…' : '立即运行完整链路'}
          </button>
          <button className="run secondary" onClick={() => void scan('扫描数据源')} disabled={busy !== null}>
            {busy === '扫描数据源' ? '扫描中…' : '扫描数据源'}
          </button>
          {OPERATIONS.map((operation) => (
            <button
              key={operation.id}
              className="run secondary"
              onClick={() => void run(operation.label, () => runDomainPackOperation(packId, operation.id))}
              disabled={busy !== null}
            >
              {busy === operation.label ? '提交中…' : operation.label}
            </button>
          ))}
        </div>
      )}

      {notice && <div className="ok-msg">{notice}</div>}
      {error && <div className="err-msg">⚠️ {error}</div>}

      {jobs.length === 0 ? (
        <div className="automation-empty">暂无 {packName} 的自动化任务。配置文件监测后，数据源变化会自动进入 sync → analyze → backtest → review 链。</div>
      ) : (
        <>
          <section className="automation-job-history" aria-label="自动化任务历史">
            <div className="automation-job-history-head">
              <div>
                <strong>任务历史</strong>
                <span>显示 {filteredJobs.length} / {jobs.length} 条</span>
              </div>
              <button
                className="text-button"
                onClick={() => {
                  setJobStatusFilter('all')
                  setJobTypeFilter('all')
                  setStrategyFilter('all')
                  setDataVersionFilter('all')
                  setJobTimeFilter('all')
                }}
                disabled={jobStatusFilter === 'all' && jobTypeFilter === 'all' && strategyFilter === 'all' && dataVersionFilter === 'all' && jobTimeFilter === 'all'}
              >
                清除筛选
              </button>
            </div>
            <div className="automation-job-filters">
              <label>任务类型
                <select aria-label="任务类型筛选" value={jobTypeFilter} onChange={(event) => setJobTypeFilter(event.target.value as JobTypeFilter)}>
                  <option value="all">全部类型</option>
                  {OPERATIONS.map((operation) => <option key={operation.id} value={operation.id}>{operation.label}</option>)}
                </select>
              </label>
              <label>状态
                <select aria-label="任务状态筛选" value={jobStatusFilter} onChange={(event) => setJobStatusFilter(event.target.value as JobStatusFilter)}>
                  <option value="all">全部状态</option>
                  <option value="queued">排队中</option>
                  <option value="running">执行中</option>
                  <option value="succeeded">成功</option>
                  <option value="failed">失败</option>
                  <option value="cancelled">已取消</option>
                </select>
              </label>
              <label>策略 ID
                <select aria-label="策略 ID 筛选" value={strategyFilter} onChange={(event) => setStrategyFilter(event.target.value)}>
                  <option value="all">全部策略</option>
                  {strategyOptions.map((strategy) => <option key={strategy} value={strategy}>{strategy}</option>)}
                </select>
              </label>
              <label>数据版本
                <select aria-label="数据版本筛选" value={dataVersionFilter} onChange={(event) => setDataVersionFilter(event.target.value)}>
                  <option value="all">全部版本</option>
                  {dataVersionOptions.map((version) => <option key={version} value={version}>{version.slice(0, 18)}</option>)}
                </select>
              </label>
              <label>时间范围
                <select aria-label="时间范围筛选" value={jobTimeFilter} onChange={(event) => setJobTimeFilter(event.target.value as JobTimeFilter)}>
                  <option value="all">全部时间</option>
                  <option value="24h">最近 24 小时</option>
                  <option value="7d">最近 7 天</option>
                  <option value="30d">最近 30 天</option>
                </select>
              </label>
            </div>
          </section>

          {errorGroups.length > 0 && (
            <section className="automation-error-groups" aria-label="历史失败任务">
              <div className="automation-error-groups-head"><strong>历史失败任务（审计保留）</strong><span>后续同阶段成功会标记为已恢复，未恢复故障优先显示。</span></div>
              <div className="automation-error-group-list">
                {errorGroups.map((group) => (
                  <article className={`automation-error-group ${group.recovered ? 'recovered' : 'active'}`} key={group.message}>
                    <div className="automation-error-group-main">
                      <span>{group.message}</span>
                      <b className={`automation-error-state ${group.recovered ? 'recovered' : 'active'}`}>{group.recovered ? '已恢复' : '仍需处理'}</b>
                    </div>
                    <div className="automation-error-group-meta">
                      <span>最后失败 {formatTime(group.latestFailedAt)}</span>
                      {group.recoveredAt && <span>恢复时间 {formatTime(group.recoveredAt)}</span>}
                      <b>历史出现 {group.count} 次</b>
                    </div>
                  </article>
                ))}
              </div>
            </section>
          )}

          {filteredJobs.length === 0 ? (
            <div className="automation-empty">没有匹配的自动化任务。</div>
          ) : (
            <div className="automation-job-list">
          {filteredJobs.map((job) => (
            <article className="automation-job" key={job.id}>
              <div className="automation-job-head">
                <div>
                  <strong>{job.job_type}</strong>
                  <code>{job.id}</code>
                </div>
                <span className={'automation-status ' + job.status}>{statusLabel(job.status)}</span>
              </div>
              <div className="automation-job-meta">
                <span>创建 {formatTime(job.created_at)}</span>
                <span>尝试 {job.attempt}/{job.max_attempts}</span>
                <span>更新 {formatTime(job.updated_at)}</span>
                {stringValue(job.payload.strategy_id) && <span>策略 <code>{stringValue(job.payload.strategy_id)}</code></span>}
                {stringValue(job.payload.input_version) && <span>版本 <code>{stringValue(job.payload.input_version)?.slice(0, 12)}</code></span>}
              </div>
              <p className="automation-job-summary">{jobSummary(job)}</p>
              {job.job_type === 'review' && job.status === 'succeeded' && reviewSummary(job) && (
                <div className="automation-review-summary">
                  <strong>Pi Agent 复核</strong>
                  <span>{reviewSummary(job)}</span>
                  <small>该结果仅解释统计与回测，不构成中奖预测。</small>
                </div>
              )}
              {job.last_error && <div className="automation-job-error">{job.last_error}</div>}
              <div className="automation-job-foot">
                <span className="muted">{job.status === 'failed' ? '任务已达到最大重试次数；修正数据或配置后可重新运行。' : job.status === 'queued' && job.attempt > 0 ? `等待重试（第 ${job.attempt} 次尝试已失败）。` : job.status === 'succeeded' ? '运行记录已写入本体世界。' : '任务状态由后端队列持续更新。'}</span>
                <div className="automation-job-foot-actions">
                  <details className="automation-job-details">
                    <summary>查看完整 JSON</summary>
                    <pre>{JSON.stringify(job, null, 2)}</pre>
                  </details>
                  {canTrigger && isCancellable(job) && (
                    <button className="run secondary" onClick={() => void cancel(job)} disabled={busy === job.id}>
                      {busy === job.id ? '取消中…' : '取消任务'}
                    </button>
                  )}
                </div>
              </div>
            </article>
          ))}
            </div>
          )}
        </>
      )}
    </section>
  )
}
