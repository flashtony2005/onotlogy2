import type { AutomationJobDto } from '../types'

interface LotteryResultPanelProps {
  jobs: AutomationJobDto[]
}

type JsonRecord = Record<string, unknown>

function recordValue(value: unknown): JsonRecord | null {
  return value && typeof value === 'object' && !Array.isArray(value) ? value as JsonRecord : null
}

function stringValue(value: unknown): string | null {
  return typeof value === 'string' && value.trim().length > 0 ? value : null
}

function numberValue(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null
}

function displayValue(value: unknown): string {
  if (typeof value === 'string' || typeof value === 'number') return String(value)
  return '—'
}

function numberList(value: unknown): number[] {
  return Array.isArray(value) ? value.filter((item): item is number => numberValue(item) !== null) : []
}

function formatNumber(value: unknown, digits = 2): string {
  const number = numberValue(value)
  return number === null ? '—' : number.toFixed(digits)
}

function formatNumbers(value: unknown): string {
  const values = numberList(value)
  return values.length === 0 ? '—' : values.map((item) => String(item).padStart(2, '0')).join(' · ')
}

function unwrapResult(job: AutomationJobDto | undefined): JsonRecord | null {
  const result = recordValue(job?.result)
  const nested = recordValue(result?.result)
  return nested ?? result
}

function latestSucceeded(jobs: AutomationJobDto[], type: string): AutomationJobDto | undefined {
  return jobs
    .filter((job) => job.job_type === type && job.status === 'succeeded')
    .sort((left, right) => right.updated_at.localeCompare(left.updated_at))[0]
}

function frequencyEntries(value: unknown): Array<[string, number]> {
  const frequency = recordValue(value)
  if (!frequency) return []
  return Object.entries(frequency)
    .map(([number, count]) => [number, numberValue(count) ?? 0] as [string, number])
    .sort((left, right) => right[1] - left[1] || Number(left[0]) - Number(right[0]))
    .slice(0, 8)
}

function FrequencyList({ label, value, accent }: { label: string; value: unknown; accent: 'red' | 'blue' }) {
  const entries = frequencyEntries(value)
  const max = Math.max(...entries.map(([, count]) => count), 1)
  return (
    <div className="lottery-frequency-block">
      <div className="lottery-result-label">{label}</div>
      {entries.length === 0 ? <span className="muted">暂无频次数据</span> : (
        <div className="lottery-frequency-list">
          {entries.map(([number, count]) => (
            <div className="lottery-frequency-row" key={number}>
              <code>{String(number).padStart(2, '0')}</code>
              <div className="lottery-frequency-track"><span className={accent} style={{ width: `${(count / max) * 100}%` }} /></div>
              <b>{count}</b>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

function Provenance({ job, result }: { job?: AutomationJobDto; result: JsonRecord | null }) {
  if (!job) return null
  const strategy = stringValue(job.payload.strategy_id)
  const version = stringValue(job.payload.input_version) ?? stringValue(job.payload.fingerprint)
  const game = stringValue(result?.game)
  return (
    <div className="lottery-result-provenance">
      {game && <span>彩种 <code>{game}</code></span>}
      {strategy && <span>策略 <code>{strategy}</code></span>}
      {version && <span>数据版本 <code>{version.slice(0, 16)}</code></span>}
      <span>任务 <code>{job.id}</code></span>
    </div>
  )
}

export function LotteryResultPanel({ jobs }: LotteryResultPanelProps) {
  const statisticsJob = latestSucceeded(jobs, 'analyze')
  const backtestJob = latestSucceeded(jobs, 'backtest')
  const reviewJob = latestSucceeded(jobs, 'review')
  const statistics = unwrapResult(statisticsJob)
  const backtest = unwrapResult(backtestJob)
  const review = unwrapResult(reviewJob)
  if (!statistics && !backtest && !review) return null

  const redBaseline = numberValue(backtest?.random_baseline_red_hits)
  const redMean = numberValue(backtest?.mean_red_hits)
  const blueBaseline = numberValue(backtest?.random_baseline_blue_hits)
  const blueMean = numberValue(backtest?.mean_blue_hits)
  const confidence = numberValue(review?.confidence)

  return (
    <section className="lottery-result-panel" aria-label="彩票分析结果">
      <div className="lottery-result-head">
        <div>
          <span className="domain-label">分析结果 · 可复核视图</span>
          <p>统计、回测和 Pi Agent 复核分栏展示；不同策略只在各自上下文中解释，不合并评分。</p>
        </div>
        <span className="lottery-result-badge">结构化结果</span>
      </div>

      <div className="lottery-result-grid">
        <article className="lottery-result-card statistics">
          <div className="lottery-result-card-head"><strong>统计分析</strong><span>{statistics ? '已完成' : '暂无结果'}</span></div>
          {statistics ? (
            <>
              <div className="lottery-result-metrics">
                <div><b>{displayValue(statistics.sample_size)}</b><span>样本期数</span></div>
                <div><b>{displayValue(statistics.window)}</b><span>训练窗口</span></div>
                <div><b>{displayValue(statistics.as_of_issue)}</b><span>截至期号</span></div>
              </div>
              <div className="lottery-candidate-line"><span>前区候选</span><strong>{formatNumbers(statistics.red_candidate)}</strong></div>
              <div className="lottery-candidate-line"><span>后区候选</span><strong>{formatNumbers(statistics.blue_candidate)}</strong></div>
              <div className="lottery-result-facts">
                <span>平均和值 <b>{formatNumber(statistics.average_red_sum)}</b></span>
                <span>平均跨度 <b>{formatNumber(statistics.average_red_span)}</b></span>
                <span>平均奇数 <b>{formatNumber(statistics.average_odd_count)}</b></span>
              </div>
              <div className="lottery-frequency-grid">
                <FrequencyList label="前区频次 Top 8" value={statistics.red_frequency} accent="red" />
                <FrequencyList label="后区频次" value={statistics.blue_frequency} accent="blue" />
              </div>
              <Provenance job={statisticsJob} result={statistics} />
            </>
          ) : <div className="lottery-result-empty">等待统计分析任务完成。</div>}
        </article>

        <article className="lottery-result-card backtest">
          <div className="lottery-result-card-head"><strong>策略回测</strong><span>{backtest ? '已完成' : '暂无结果'}</span></div>
          {backtest ? (
            <>
              <div className="lottery-result-metrics">
                <div><b>{displayValue(backtest.target_count)}</b><span>测试期数</span></div>
                <div><b>{displayValue(backtest.train_window)}</b><span>训练窗口</span></div>
                <div><b>{displayValue(backtest.max_red_hits)}</b><span>最高前区命中</span></div>
              </div>
              <div className="lottery-backtest-table">
                <div><span>前区平均命中</span><b>{formatNumber(backtest.mean_red_hits)}</b><small>随机基线 {formatNumber(redBaseline)}</small><em className={redMean !== null && redBaseline !== null && redMean >= redBaseline ? 'positive' : 'neutral'}>{redMean !== null && redBaseline !== null ? `${redMean >= redBaseline ? '+' : ''}${(redMean - redBaseline).toFixed(2)}` : '—'}</em></div>
                <div><span>后区平均命中</span><b>{formatNumber(backtest.mean_blue_hits)}</b><small>随机基线 {formatNumber(blueBaseline)}</small><em className={blueMean !== null && blueBaseline !== null && blueMean >= blueBaseline ? 'positive' : 'neutral'}>{blueMean !== null && blueBaseline !== null ? `${blueMean >= blueBaseline ? '+' : ''}${(blueMean - blueBaseline).toFixed(2)}` : '—'}</em></div>
              </div>
              <div className="lottery-result-disclaimer">回测只描述历史样本表现，不代表未来中奖概率。</div>
              <Provenance job={backtestJob} result={backtest} />
            </>
          ) : <div className="lottery-result-empty">等待回测任务完成。</div>}
        </article>

        <article className="lottery-result-card review">
          <div className="lottery-result-card-head"><strong>Pi Agent 复核</strong><span>{review ? '已完成' : '暂无结果'}</span></div>
          {review ? (
            <>
              <div className="lottery-review-score"><span>复核置信度</span><strong>{confidence === null ? '—' : `${(confidence * 100).toFixed(0)}%`}</strong></div>
              <div className="lottery-review-capability"><span>建议能力</span><code>{stringValue(review.capability) ?? '无需追加动作'}</code></div>
              <p className="lottery-review-reasoning">{stringValue(review.reasoning) ?? 'Pi Agent 未提供额外解释。'}</p>
              {recordValue(review.args) && <details><summary>查看结构化参数</summary><pre>{JSON.stringify(review.args, null, 2)}</pre></details>}
              <div className="lottery-result-disclaimer">Pi Agent 只解释统计与回测，不构成中奖预测或投注建议。</div>
              <Provenance job={reviewJob} result={review} />
            </>
          ) : <div className="lottery-result-empty">等待 Pi Agent 复核任务完成。</div>}
        </article>
      </div>
    </section>
  )
}
