import { useEffect, useMemo, useState } from 'react'
import {
  AuthError,
  getEvolution,
  getMe,
  getMemory,
  getTasks,
  postLogin,
  postLogout,
  postRun,
} from './api'
import { EvolutionPanel } from './components/EvolutionPanel'
import { LoginForm } from './components/LoginForm'
import { MemoryPanel } from './components/MemoryPanel'
import { TaskPanel } from './components/TaskPanel'
import { V2Panel } from './components/V2Panel'
import { debounce, fmtJson, fmtTime } from './format'
import { clearToken, getToken, setToken } from './auth'
import { useTraceStream } from './hooks/useTraceStream'
import { buildTimeline } from './view'
import type { AuthUserDto, EvolutionStateDto, MemoryRecord, TaskRecord, TraceEventDto, ViewKind } from './types'

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
  { key: 'tasks', label: 'Tasks', desc: '任务状态 · Pending→Completed' },
  { key: 'v2', label: 'V2 Runtime', desc: 'Skill · Plugin · Session（Phase 9-10）' },
]

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

  // ── V0.7 认证：会话恢复 + 登录 + 登出 ──────────────────────────────────
  const [user, setUser] = useState<AuthUserDto | null>(null)
  const [sessionChecking, setSessionChecking] = useState(true)
  const [loginError, setLoginError] = useState<string | null>(null)

  // 刷新/重开页面：用备份令牌调 /auth/me 恢复会话（无效则清令牌回登录页）
  useEffect(() => {
    let cancelled = false
    ;(async () => {
      if (!getToken()) {
        if (!cancelled) setSessionChecking(false)
        return
      }
      try {
        const me = await getMe()
        if (!cancelled) setUser(me)
      } catch {
        clearToken()
      } finally {
        if (!cancelled) setSessionChecking(false)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [])

  /** 统一 401 处理：清令牌并回登录页。 */
  const handleAuthError = (e: unknown) => {
    if (e instanceof AuthError) {
      clearToken()
      setUser(null)
      setRunId(null)
      setEvents([])
      return true
    }
    return false
  }

  const doLogin = async (username: string, password: string) => {
    setLoginError(null)
    try {
      const { token, user: u } = await postLogin(username, password)
      setToken(token)
      setUser(u)
    } catch (e) {
      setLoginError((e as Error).message || '登录失败')
    }
  }

  const doLogout = async () => {
    try {
      await postLogout()
    } catch {
      /* 令牌已失效也照常清本地 */
    }
    clearToken()
    setUser(null)
    setRunId(null)
    setEvents([])
  }

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
  const [tasks, setTasks] = useState<TaskRecord[]>([])
  const [tasksLoading, setTasksLoading] = useState(false)

  const reloadTasks = async () => {
    setTasksLoading(true)
    try {
      const dto = await getTasks()
      setTasks(dto.list)
    } catch {
      /* 暂无任务或后端未就绪：保留上次结果 */
    } finally {
      setTasksLoading(false)
    }
  }

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

  // 节流版刷新（useMemo 保持单例；内部只依赖稳定 setState，捕获旧闭包无碍）
  const reloadMemoryDebounced = useMemo(() => debounce(() => void reloadMemory(), 300), [])
  const reloadTasksDebounced = useMemo(() => debounce(() => void reloadTasks(), 300), [])

  useTraceStream(runId, {
    onEvent: (e) => {
      setEvents((prev) => (prev.some((p) => p.id === e.id) ? prev : [...prev, e]))
      // 记忆沉淀/召回事件 → 实时刷新 Memory 视图（hits 累加可见）
      // U4：事件高频到达时合并刷新（300ms 窗口一次），避免并发 REST 互相覆盖
      if (e.eventType === 'MemoryUpdate' || e.eventType === 'MemoryRecall') {
        reloadMemoryDebounced()
      }
      if (e.eventType === 'Action' || e.eventType === 'Planning') {
        reloadTasksDebounced()
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
      void reloadMemory()
      void reloadTasks()
    } catch (e) {
      if (!handleAuthError(e)) setError((e as Error).message)
    } finally {
      setRunning(false)
    }
  }

  // 未登录：渲染登录页（V0.7 认证强制）
  if (sessionChecking) {
    return (
      <div className="app">
        <header className="topbar">
          <div className="brand">
            <span className="logo">◈</span>
            <div>
              <h1>OpenAIP-OS · Agent 认知观察</h1>
              <p className="sub">V0.8 · 本地账号认证</p>
            </div>
          </div>
        </header>
        <main className="timeline">
          <div className="empty">正在恢复会话…</div>
        </main>
      </div>
    )
  }

  if (!user) {
    return (
      <div className="app">
        <header className="topbar">
          <div className="brand">
            <span className="logo">◈</span>
            <div>
              <h1>OpenAIP-OS · Agent 认知观察</h1>
              <p className="sub">V0.8 · 本地账号认证</p>
            </div>
          </div>
        </header>
        <main className="timeline">
          <LoginForm error={loginError} onLogin={doLogin} />
        </main>
      </div>
    )
  }

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <span className="logo">◈</span>
          <div>
            <h1>OpenAIP-OS · Agent 认知观察</h1>
            <p className="sub">V0.8 · GraphRAG 基础 · 认证 · 声明式 Action</p>
          </div>
        </div>
        <div className="conn-group">
          <div className={`conn ${connected ? 'on' : 'off'}`}>
            <span className="dot" />
            {connected ? 'WS 已连接' : 'WS 未连接'}
          </div>
          <div className="user">
            <span className="user-name">{user.username}</span>
            <span className={`role role-${user.role.toLowerCase()}`}>{user.role}</span>
            <button className="logout" onClick={() => void doLogout()}>
              登出
            </button>
          </div>
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
              if (v.key === 'tasks') void reloadTasks()
            }}
            title={v.desc}
          >
            <b>{v.label}</b>
            <small>{v.desc}</small>
          </button>
        ))}
      </nav>

      <main className="timeline">
        {!runId && view !== 'memory' && view !== 'evolution' && view !== 'tasks' && view !== 'v2' && (
          <div className="empty">
            输入目标并点击「触发 Agent 运行」，实时观察 Agent 认知闭环。
          </div>
        )}

        {view === 'memory' && (
          <MemoryPanel memories={memories} loading={memoryLoading} onReload={reloadMemory} />
        )}

        {view === 'v2' && <V2Panel />}

        {view === 'evolution' && (
          <EvolutionPanel
            state={evolution}
            loading={evolutionLoading}
            onReload={reloadEvolution}
            onAuthError={() => {
              clearToken()
              setUser(null)
              setRunId(null)
              setEvents([])
            }}
          />
        )}

        {view === 'tasks' && (
          <TaskPanel tasks={tasks} loading={tasksLoading} onReload={reloadTasks} />
        )}

        {runId && timeline.decision && view !== 'memory' && view !== 'evolution' && view !== 'tasks' && view !== 'v2' && (
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

        {runId && view !== 'memory' && view !== 'evolution' && view !== 'tasks' && view !== 'v2' && timeline.events.length === 0 && (
          <div className="empty">等待认知事件…</div>
        )}

        {view !== 'memory' && view !== 'evolution' && view !== 'tasks' && view !== 'v2' && timeline.events.length > 0 &&
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
