import { useMemo, useState } from 'react'
import { getApprovalAudit, getEvolution, getMemory, postAutoApproveToggle, postRun } from './api'
import { useTraceStream } from './hooks/useTraceStream'
import { buildTimeline } from './view'
import type {
  ApprovalAuditDto,
  EvolutionStateDto,
  MemoryRecord,
  TraceEventDto,
  ViewKind,
} from './types'

const STEP_ICON: Record<string, string> = {
  Goal: '🎯',
  Intent: '🧭',
  OntologyQuery: '🌐',
  Reasoning: '🧠',
  Planning: '📋',
  PolicyCheck: '🔐',
  Action: '⚡',
  MemoryUpdate: '💾',
  MemoryRecall: '🔎',
}

const VIEWS: { key: ViewKind; label: string; desc: string }[] = [
  { key: 'executive', label: 'Executive', desc: '管理者 · Mission/Decision/Risk' },
  { key: 'operator', label: 'Operator', desc: '管理员 · Pipeline 进度' },
  { key: 'developer', label: 'Developer', desc: '开发者 · 全量 IO' },
  { key: 'memory', label: 'Memory', desc: '记忆沉淀 · Action→Memory 闭环' },
  { key: 'evolution', label: 'Evolution', desc: '自学习 · Experience→Knowledge 闭环' },
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
  const [memories, setMemories] = useState<MemoryRecord[]>([])
  const [memoryLoading, setMemoryLoading] = useState(false)

  // Memory 视图刷新：从 REST /memory 拉取沉淀（hits 随轮次累加）。
  const reloadMemory = async () => {
    setMemoryLoading(true)
    try {
      const dto = await getMemory()
      setMemories(dto.list)
    } catch {
      /* 后端无记忆或暂未就绪：保留上次结果 */
    } finally {
      setMemoryLoading(false)
    }
  }

  const [evolution, setEvolution] = useState<EvolutionStateDto | null>(null)
  const [evolutionLoading, setEvolutionLoading] = useState(false)

  // Evolution 视图刷新：从 REST /evolution 拉取聚合状态（仅可见性，不改 Policy）。
  const reloadEvolution = async () => {
    setEvolutionLoading(true)
    try {
      const dto = await getEvolution()
      setEvolution(dto)
    } catch {
      /* 后端无知识或暂未就绪：保留上次结果 */
    } finally {
      setEvolutionLoading(false)
    }
  }

  useTraceStream(runId, {
    onEvent: (e) => {
      setEvents((prev) => (prev.some((p) => p.id === e.id) ? prev : [...prev, e]))
      // 记忆沉淀/召回事件 → 实时刷新 Memory 视图（hits 累加可见）
      if (e.eventType === 'MemoryUpdate' || e.eventType === 'MemoryRecall') {
        void reloadMemory()
      }
    },
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
      void reloadMemory() // 运行前先载入已有记忆（MemoryRecall 起点）
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
            <p className="sub">V0.5 · 自有 Planner 闭环 · Memory 沉淀 · Evolution(Learn≠Permission)</p>
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
            onClick={() => {
              setView(v.key)
              if (v.key === 'memory') void reloadMemory()
              if (v.key === 'evolution') void reloadEvolution()
            }}
            title={v.desc}
          >
            <b>{v.label}</b>
            <small>{v.desc}</small>
          </button>
        ))}
      </nav>

      <main className="timeline">
        {!runId && view !== 'memory' && view !== 'evolution' && (
          <div className="empty">
            输入目标并点击「触发 Agent 运行」，实时观察 Agent 认知闭环。
          </div>
        )}

        {view === 'memory' && (
          <MemoryPanel memories={memories} loading={memoryLoading} onReload={reloadMemory} />
        )}

        {view === 'evolution' && (
          <EvolutionPanel state={evolution} loading={evolutionLoading} onReload={reloadEvolution} />
        )}

        {runId && timeline.decision && view !== 'memory' && view !== 'evolution' && (
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

        {runId && view !== 'memory' && view !== 'evolution' && timeline.events.length === 0 && (
          <div className="empty">等待认知事件…</div>
        )}

        {view !== 'memory' && view !== 'evolution' && timeline.events.length > 0 &&
          timeline.events.map((ev) => (
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

// ── Memory 视图：记忆沉淀面板（V0.5 / BL-008）──────────────────────────────────
function MemoryPanel({
  memories,
  loading,
  onReload,
}: {
  memories: MemoryRecord[]
  loading: boolean
  onReload: () => void
}) {
  return (
    <div className="memory">
      <div className="memory-head">
        <h3>💾 记忆沉淀（Action → Memory → 下次 Plan 闭环）</h3>
        <button className="reload" onClick={onReload} disabled={loading}>
          {loading ? '刷新中…' : '↻ 刷新'}
        </button>
      </div>
      {memories.length === 0 ? (
        <div className="empty">
          暂无记忆。运行含动作执行的 Agent（如挽留计划）后，动作会按
          <code>action.&lt;name&gt;</code> 沉淀为记忆，hits 随复用轮次累加。
        </div>
      ) : (
        <div className="memory-list">
          {memories.map((m) => (
            <article key={m.label} className="memory-item">
              <div className="mem-head">
                <code className="mem-label">{m.label}</code>
                <span className="mem-hits" title="复用次数（自学习指标）">
                  复用 {m.hits}
                </span>
                <span className="mem-time">{fmtTime(m.updatedAt)}</span>
              </div>
              <pre className="mem-payload">{fmtJson(m.payload)}</pre>
            </article>
          ))}
        </div>
      )}
    </div>
  )
}

// ── Evolution 视图：自学习知识面板（V0.5 / BL-003 · REQ-017，Learn≠Permission；V0.6 / REQ-018 AutoApprove）──
function EvolutionPanel({
  state,
  loading,
  onReload,
}: {
  state: EvolutionStateDto | null
  loading: boolean
  onReload: () => void
}) {
  const [audits, setAudits] = useState<ApprovalAuditDto[] | null>(null)
  const [busyId, setBusyId] = useState<string | null>(null)

  const total =
    (state?.patterns.length ?? 0) +
    (state?.proposals.length ?? 0) +
    (state?.learned.length ?? 0)

  // 切换自动放行资格（REQ-018 可撤销，幂等）；成功后刷新 Evolution + 审计。
  const toggleAutoApprove = async (id: string, enabled: boolean) => {
    setBusyId(id)
    try {
      await postAutoApproveToggle(id, enabled)
      await onReload()
      const a = await getApprovalAudit()
      setAudits(a)
    } finally {
      setBusyId(null)
    }
  }

  const showAudit = async () => {
    const a = await getApprovalAudit()
    setAudits(a)
  }

  return (
    <div className="evolution">
      <div className="evo-head">
        <h3>🧬 Evolution · Experience→Knowledge 闭环</h3>
        <button className="reload" onClick={onReload} disabled={loading}>
          {loading ? '刷新中…' : '↻ 刷新'}
        </button>
      </div>
      <p className="evo-note">
        Learn≠Permission：本视图仅暴露知识，<b>不修改 Policy、不绕过 Human Approval（WB2005）</b>。
        证据链 Learned→Proposal→Pattern→Experience 全程可追溯。
      </p>

      {total === 0 ? (
        <div className="empty">
          暂无知识。运行多轮含复现动作的 Agent 后，经验会经 wb_memory 提炼为
          Pattern（hits≥2）→ Proposal → Validated Knowledge。
        </div>
      ) : (
        <div className="evo-body">
          <section className="evo-section">
            <h4>
              Pattern <span className="evo-count">{state?.patterns.length ?? 0}</span>
            </h4>
            {(state?.patterns ?? []).map((p) => (
              <article key={p.label} className="evo-item">
                <div className="evo-line">
                  <code className="evo-label">{p.label}</code>
                  <span className="evo-tag action">action: {p.action}</span>
                  <span className="evo-tag conf">conf {p.confidence.toFixed(2)}</span>
                </div>
                <div className="evo-evidence">
                  来源 run：{p.runIds.length ? p.runIds.join(', ') : '—'}
                </div>
              </article>
            ))}
          </section>

          <section className="evo-section">
            <h4>
              Proposal <span className="evo-count">{state?.proposals.length ?? 0}</span>
            </h4>
            {(state?.proposals ?? []).map((p) => (
              <article key={p.id} className="evo-item">
                <div className="evo-line">
                  <code className="evo-label">{p.id}</code>
                  <span className={`evo-tag status ${p.status.toLowerCase()}`}>{p.status}</span>
                  <span className="evo-tag conf">conf {p.confidence.toFixed(2)}</span>
                </div>
                <div className="evo-evidence">
                  来源 Pattern：{p.patternLabel} · run：
                  {p.runIds.length ? p.runIds.join(', ') : '—'}
                </div>
              </article>
            ))}
          </section>

          <section className="evo-section">
            <h4>
              Learned <span className="evo-count">{state?.learned.length ?? 0}</span>
            </h4>
            {(state?.learned ?? []).map((l) => (
              <article key={l.id} className="evo-item">
                <div className="evo-line">
                  <code className="evo-label">{l.id}</code>
                  <span className="evo-tag action">action: {l.action}</span>
                  <span className="evo-tag conf">conf {l.confidence.toFixed(2)}</span>
                  <span className={`evo-tag ${l.autoApproveEnabled ? 'aa-on' : 'aa-off'}`}>
                    {l.autoApproveEnabled ? '自动放行:开' : '自动放行:关'}
                  </span>
                  <button
                    className="evo-toggle"
                    disabled={busyId === l.id}
                    onClick={() => toggleAutoApprove(l.id, !l.autoApproveEnabled)}
                  >
                    {l.autoApproveEnabled ? '撤销' : '恢复'}
                  </button>
                </div>
                <div className="evo-evidence">
                  来源 Proposal：{l.proposalId} · run：
                  {l.runIds.length ? l.runIds.join(', ') : '—'}
                </div>
              </article>
            ))}
            <button className="reload" onClick={showAudit} style={{ marginTop: 8 }}>
              查看自动放行审计
            </button>
            {audits && (
              <div className="evo-audit">
                {audits.length === 0 ? (
                  <div className="empty">暂无自动放行审计</div>
                ) : (
                  audits.map((a) => (
                    <article key={a.id} className="evo-item">
                      <div className="evo-line">
                        <code className="evo-label">{a.action}</code>
                        <span className={`evo-tag decision ${a.decision}`}>{a.decision}</span>
                        <span className="evo-tag">run: {a.runId || '—'}</span>
                      </div>
                      <div className="evo-evidence">
                        {a.reason} · {a.createdAt}
                      </div>
                    </article>
                  ))
                )}
              </div>
            )}
          </section>
        </div>
      )}
    </div>
  )
}
