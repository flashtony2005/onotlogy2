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
import { Dashboard } from './components/Dashboard'
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

const VIEW_GROUPS: { name: string; items: ViewKind[] }[] = [
  { name: '总览', items: ['overview'] },
  {
    name: 'Agent 观察',
    items: ['executive', 'operator', 'developer'],
  },
  { name: '认知', items: ['memory', 'evolution', 'tasks'] },
  { name: '能力', items: ['v2'] },
]

const VIEWS: { key: ViewKind; label: string; icon: string; desc: string }[] = [
  { key: 'overview', label: 'Overview', icon: '◎', desc: '系统总览 · 指标/架构/活动' },
  { key: 'executive', label: 'Executive', icon: '◈', desc: '管理者 · 决策 / 风险' },
  { key: 'operator', label: 'Operator', icon: '▶', desc: '操作台 · 流水线进度' },
  { key: 'developer', label: 'Developer', icon: '</>', desc: '开发者 · 全量 IO' },
  { key: 'memory', label: 'Memory', icon: '💾', desc: '记忆沉淀 · 动作→记忆闭环' },
  { key: 'evolution', label: 'Evolution', icon: '◉', desc: '自学习 · 经验→知识' },
  { key: 'tasks', label: 'Tasks', icon: '☑', desc: '任务状态 · 待办→完成' },
  { key: 'v2', label: 'V2 Runtime', icon: '▣', desc: 'Skill · Plugin · Session' },
]

export default function App() {
  const [view, setView] = useState<ViewKind>('overview')
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

  // WS 订阅：有 run 时跟 run 流；无 run 时连 agent 流（登录即连接，run 启动后事件实时到达）
  useTraceStream(runId ?? agentId, {
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
      <div className="app-shell">
        <div className="login-wrap">
          <div className="empty">❯ 正在恢复会话…</div>
        </div>
      </div>
    )
  }

  if (!user) {
    return (
      <div className="app-shell">
        <div className="login-wrap">
          <div className="login-brand">
            <span className="logo">❯_</span> OpenAIP-OS
          </div>
          <LoginForm error={loginError} onLogin={doLogin} />
        </div>
      </div>
    )
  }

  return (
    <div className="app-shell">
      {/* Codex 式左侧边栏：品牌 + 分组导航 + 状态/登出 */}
      <aside className="sidebar">
        <div className="side-brand">
          <span className="side-logo">❯_</span>
          <span className="side-title">OpenAIP-OS</span>
        </div>

        <nav className="side-nav">
          {VIEW_GROUPS.map((g) => (
            <div key={g.name} className="side-group">
              <div className="side-group-name">{g.name}</div>
              {g.items.map((k) => {
                const v = VIEWS.find((x) => x.key === k)!
                return (
                  <button
                    key={k}
                    className={view === k ? 'side-btn active' : 'side-btn'}
                    onClick={() => {
                      setView(k)
                      if (k === 'memory') void reloadMemory()
                      if (k === 'evolution') void reloadEvolution()
                      if (k === 'tasks') void reloadTasks()
                    }}
                    title={v.desc}
                  >
                    <span className="side-ic">{v.icon}</span>
                    <span className="side-label">{v.label}</span>
                    <span className="side-desc">{v.desc}</span>
                  </button>
                )
              })}
            </div>
          ))}
        </nav>

        <div className="side-bottom">
          <div className={`side-conn ${connected ? 'on' : 'off'}`}>
            <span className="dot" />
            <span>{connected ? 'WS 已连接' : 'WS 未连接'}</span>
          </div>
          <button className="side-btn side-logout" onClick={() => void doLogout()}>
            <span className="side-ic">⎋</span>
            <span className="side-label">登出</span>
          </button>
        </div>
      </aside>

      <div className="main-col">
        {/* 顶部 mini bar：会话上下文 + 连接状态 */}
        <header className="mini-bar">
          <div className="mini-title">
            <span className="logo">❯_</span>
            <span className="mini-name">OpenAIP-OS</span>
            <span className="mini-sub">{VIEWS.find((v) => v.key === view)?.label ?? ''}</span>
          </div>
          <div className="conn-group">
            <div className={`conn ${connected ? 'on' : 'off'}`}>
              <span className="dot" />
              {connected ? 'WS 已连接' : 'WS 未连接'}
            </div>
            <div className="user">
              <span className="user-name">{user.username}</span>
              <span className={`role role-${user.role.toLowerCase()}`}>{user.role}</span>
            </div>
          </div>
        </header>

        {runId && (
          <section className="status">
            <div className="meta">
              <span>
                run <code>{runId}</code>
              </span>
              <span>
                状态 <b className={timeline.status.toLowerCase()}>{timeline.status}</b>
              </span>
              <span>审批 {timeline.humanApproval}</span>
            </div>
            <div className="progress">
              <div className="bar" style={{ width: `${timeline.progress}%` }} />
              <span>{timeline.progress}%</span>
            </div>
          </section>
        )}

        {error && <div className="error">⚠ {error}</div>}

        <main className="content">
        {!runId && view !== 'overview' && view !== 'memory' && view !== 'evolution' && view !== 'tasks' && view !== 'v2' && (
          <div className="empty">
            输入目标并点击「触发 Agent 运行」，实时观察 Agent 认知闭环。
          </div>
        )}

        {view === 'memory' && (
          <MemoryPanel memories={memories} loading={memoryLoading} onReload={reloadMemory} />
        )}

        {view === 'overview' && <Dashboard onNavigate={(v) => setView(v)} />}

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

        {runId && timeline.decision && view !== 'overview' && view !== 'memory' && view !== 'evolution' && view !== 'tasks' && view !== 'v2' && (
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

        {runId && view !== 'overview' && view !== 'memory' && view !== 'evolution' && view !== 'tasks' && view !== 'v2' && timeline.events.length === 0 && (
          <div className="empty">等待认知事件…</div>
        )}

        {view !== 'overview' && view !== 'memory' && view !== 'evolution' && view !== 'tasks' && view !== 'v2' && timeline.events.length > 0 &&
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

        {/* Codex 式底部 composer */}
        <footer className="composer">
          <span className="prompt">❯</span>
          <input
            className="agent"
            value={agentId}
            onChange={(e) => setAgentId(e.target.value)}
            placeholder="agent"
            title="Agent ID"
          />
          <input
            className="goal"
            value={goal}
            onChange={(e) => setGoal(e.target.value)}
            placeholder="自然语言目标（goal），Enter 触发"
            onKeyDown={(e) => e.key === 'Enter' && startRun()}
          />
          <button className="run" onClick={startRun} disabled={running}>
            {running ? '…' : '▶'}
          </button>
        </footer>
      </div>
    </div>
  )
}
