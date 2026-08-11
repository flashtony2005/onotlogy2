import { useMemo, useState } from 'react'
import { postRun } from './api'
import { useTraceStream } from './hooks/useTraceStream'
import { buildTimeline } from './view'
import type { TraceEventDto, ViewKind } from './types'

const STEP_ICON: Record<string, string> = {
  Goal: '🎯',
  Intent: '🧭',
  OntologyQuery: '🌐',
  Reasoning: '🧠',
  Planning: '📋',
  PolicyCheck: '🔐',
  Action: '⚡',
  MemoryUpdate: '💾',
}

const VIEWS: { key: ViewKind; label: string; desc: string }[] = [
  { key: 'executive', label: 'Executive', desc: '管理者 · Mission/Decision/Risk' },
  { key: 'operator', label: 'Operator', desc: '管理员 · Pipeline 进度' },
  { key: 'developer', label: 'Developer', desc: '开发者 · 全量 IO' },
]

function fmtTime(ts: string): string {
  const d = new Date(ts)
  if (isNaN(d.getTime())) return ts
  return d.toLocaleTimeString('zh-CN', { hour12: false })
}

function fmtJson(v: unknown): string {
  if (v == null) return ''
  if (typeof v === 'string') return v
  try {
    return JSON.stringify(v, null, 2)
  } catch {
    return String(v)
  }
}

export default function App() {
  const [view, setView] = useState<ViewKind>('operator')
  const [agentId, setAgentId] = useState('retention-agent')
  const [goal, setGoal] = useState('为高风险流失客户生成挽留计划')
  const [runId, setRunId] = useState<string | null>(null)
  const [events, setEvents] = useState<TraceEventDto[]>([])
  const [connected, setConnected] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [running, setRunning] = useState(false)

  useTraceStream(runId, {
    onEvent: (e) =>
      setEvents((prev) => (prev.some((p) => p.id === e.id) ? prev : [...prev, e])),
    onStatus: setConnected,
  })

  const timeline = useMemo(
    () => buildTimeline(events, runId ?? '', agentId, view),
    [events, runId, agentId, view],
  )

  const startRun = async () => {
    setError(null)
    if (!goal.trim()) {
      setError('goal 必填')
      return
    }
    setRunning(true)
    try {
      const dto = await postRun(agentId, goal)
      setEvents([])
      setRunId(dto.runId)
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setRunning(false)
    }
  }

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <span className="logo">◈</span>
          <div>
            <h1>OpenAIP-OS · Agent 认知观察</h1>
            <p className="sub">V0.3 · Pi 作 Cognitive Engine · 自有 Planner 闭环</p>
          </div>
        </div>
        <div className={`conn ${connected ? 'on' : 'off'}`}>
          <span className="dot" />
          {connected ? 'WS 已连接' : 'WS 未连接'}
        </div>
      </header>

      <section className="controls">
        <input
          className="agent"
          value={agentId}
          onChange={(e) => setAgentId(e.target.value)}
          placeholder="Agent ID"
        />
        <input
          className="goal"
          value={goal}
          onChange={(e) => setGoal(e.target.value)}
          placeholder="自然语言目标（goal）"
          onKeyDown={(e) => e.key === 'Enter' && startRun()}
        />
        <button className="run" onClick={startRun} disabled={running}>
          {running ? '触发中…' : '▶ 触发 Agent 运行'}
        </button>
      </section>

      {error && <div className="error">⚠ {error}</div>}

      {runId && (
        <section className="status">
          <div className="meta">
            <span>
              Run: <code>{runId}</code>
            </span>
            <span>
              状态: <b className={timeline.status.toLowerCase()}>{timeline.status}</b>
            </span>
            <span>审批: {timeline.humanApproval}</span>
          </div>
          <div className="progress">
            <div className="bar" style={{ width: `${timeline.progress}%` }} />
            <span>{timeline.progress}%</span>
          </div>
        </section>
      )}

      <nav className="views">
        {VIEWS.map((v) => (
          <button
            key={v.key}
            className={view === v.key ? 'active' : ''}
            onClick={() => setView(v.key)}
            title={v.desc}
          >
            <b>{v.label}</b>
            <small>{v.desc}</small>
          </button>
        ))}
      </nav>

      <main className="timeline">
        {!runId && (
          <div className="empty">
            输入目标并点击「触发 Agent 运行」，实时观察 Agent 认知闭环。
          </div>
        )}

        {runId && timeline.decision && (
          <div className="decision">
            <h3>📊 决策摘要（Executive）</h3>
            <div>
              <span>使命</span>
              <p>{timeline.decision.mission}</p>
            </div>
            <div>
              <span>决策</span>
              <p>{timeline.decision.decision}</p>
            </div>
            <div>
              <span>风险</span>
              <p>{timeline.decision.risk}</p>
            </div>
          </div>
        )}

        {runId && timeline.events.length === 0 && (
          <div className="empty">等待认知事件…</div>
        )}

        {timeline.events.map((ev) => (
          <article key={ev.id} className={`event ${ev.eventType.toLowerCase()}`}>
            <div className="ev-head">
              <span className="ic">{STEP_ICON[ev.eventType] ?? '•'}</span>
              <span className="et">{ev.eventType}</span>
              <span className="ts">{fmtTime(ev.timestamp)}</span>
              {ev.confidence != null && (
                <span className="cf">置信 {ev.confidence.toFixed(2)}</span>
              )}
            </div>
            {ev.objectRefs && ev.objectRefs.length > 0 && (
              <div className="refs">
                {ev.objectRefs.map((r) => (
                  <code key={r} className="chip">
                    {r}
                  </code>
                ))}
              </div>
            )}
            {ev.reasoning && <pre className="reasoning">{ev.reasoning}</pre>}
            {ev.input != null && (
              <div className="io">
                <label>input</label>
                <pre>{fmtJson(ev.input)}</pre>
              </div>
            )}
            {ev.output != null && (
              <div className="io">
                <label>output</label>
                <pre>{fmtJson(ev.output)}</pre>
              </div>
            )}
          </article>
        ))}
      </main>
    </div>
  )
}
