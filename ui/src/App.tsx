import { useState, useEffect, useMemo } from 'react'
import {
  Boxes,
  Target,
  Globe2,
  Cpu,
  LogOut,
  Sparkles,
  Play,
} from 'lucide-react'
import {
  getMe,
  postLogin,
  postLogout,
  postRun,
} from './api'
import { LoginForm } from './components/LoginForm'
import { LoginScene } from './components/LoginScene'
import { ExecutionSpace } from './components/ExecutionSpace'
import { WorldSpace } from './components/WorldSpace'
import { FabricSpace } from './components/FabricSpace'
import { getToken, setToken, clearToken } from './auth'
import { useTraceStream } from './hooks/useTraceStream'
import { buildTimeline } from './view'
import type { AuthUserDto, TraceEventDto } from './types'

export type PureSpace = 'execution' | 'world' | 'fabric'

export default function App() {
  const [space, setSpace] = useState<PureSpace>('execution')
  const [agentId, setAgentId] = useState('retention-agent')
  const [goal, setGoal] = useState('为高风险流失客户生成挽留计划')
  const [runId, setRunId] = useState<string | null>(null)
  const [events, setEvents] = useState<TraceEventDto[]>([])
  const [connected, setConnected] = useState(true)
  const [running, setRunning] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // 认证状态
  const [user, setUser] = useState<AuthUserDto | null>(null)
  const [sessionChecking, setSessionChecking] = useState(true)
  const [loginError, setLoginError] = useState<string | null>(null)

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

  const doLogin = async (username: string, password?: string) => {
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
      /* ignore */
    }
    clearToken()
    setUser(null)
    setRunId(null)
    setEvents([])
  }

  // 实时轨迹流订阅
  useTraceStream(runId ?? agentId, {
    enabled: !!user,
    onEvent: (e) => {
      setEvents((prev) => (prev.some((p) => p.id === e.id) ? prev : [...prev, e]))
    },
    onStatus: setConnected,
  })

  const timeline = useMemo(
    () => buildTimeline(events, runId ?? '', agentId, 'overview' as any),
    [events, runId, agentId],
  )

  const handleTriggerRun = async (targetGoal?: string) => {
    const finalGoal = (targetGoal || goal).trim()
    if (!finalGoal) {
      setError('目标 (Goal) 必填')
      return
    }
    setError(null)
    setRunning(true)
    if (space !== 'execution') {
      setSpace('execution')
    }
    try {
      const res = await postRun(agentId, finalGoal)
      setEvents([])
      setRunId(res.runId)
      if (targetGoal) setGoal(targetGoal)
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setRunning(false)
    }
  }

  const missionPresets = [
    { label: '🎯 客户流失挽留方案', goal: '对近14天活跃下降的高价值客户自动制定专属挽留方案' },
    { label: '📈 业务规律分析回测', goal: '分析历史业务数据特征并测算未来指标变化走势' },
    { label: '🛡️ 业务数据与规则体检', goal: '全面体检当前业务数据完整性与规则逻辑一致性' },
  ]

  if (sessionChecking) {
    return (
      <div className="auth-page">
        <div className="auth-empty">❯ 正在载入工作台…</div>
      </div>
    )
  }

  if (!user) {
    return (
      <div className="auth-page">
        <nav className="auth-nav">
          <div className="auth-brand">
            <span className="auth-brand-leaf">🌿</span> OpenAIP-OS · 智能业务协同工作台
          </div>
          <div className="auth-nav-links">
            <span style={{ fontSize: '12px', color: '#6d857b' }}>简单明了 · 高效协同</span>
          </div>
        </nav>

        <div className="auth-scene">
          <LoginScene />
        </div>

        <div className="auth-form-wrap">
          <LoginForm error={loginError} onLogin={doLogin} />
        </div>
      </div>
    )
  }

  return (
    <div className="zen-app">
      {/* ── 顶部极简导航 ──────────────────────────────────────────────── */}
      <header className="zen-header">
        <div className="zen-brand">
          <span className="zen-logo">
            <Boxes size={18} strokeWidth={2.4} />
          </span>
          <div className="zen-brand-info">
            <span className="zen-title">OpenAIP-OS</span>
            <span className="zen-sub">智能业务协同平台</span>
          </div>
        </div>

        {/* 核心工作空间选择器 */}
        <nav className="zen-spaces">
          <button
            className={`space-nav-btn ${space === 'execution' ? 'active' : ''}`}
            onClick={() => setSpace('execution')}
          >
            <Target size={16} />
            <span>业务任务</span>
            <span className="sub-tag">Tasks</span>
          </button>

          <button
            className={`space-nav-btn ${space === 'world' ? 'active' : ''}`}
            onClick={() => setSpace('world')}
          >
            <Globe2 size={16} />
            <span>业务知识</span>
            <span className="sub-tag">Knowledge</span>
          </button>

          <button
            className={`space-nav-btn ${space === 'fabric' ? 'active' : ''}`}
            onClick={() => setSpace('fabric')}
          >
            <Cpu size={16} />
            <span>团队与工具</span>
            <span className="sub-tag">Agents & Tools</span>
          </button>
        </nav>

        {/* 用户与连接状态 */}
        <div className="zen-user-group">
          <div className={`conn-status ${connected ? 'live' : 'offline'}`}>
            <span className="dot" />
            <span>{connected ? '实时连接正常' : '连接中断'}</span>
          </div>

          <div className="zen-user-badge">
            <span className="avatar">{user.username.slice(0, 1).toUpperCase()}</span>
            <div className="meta">
              <span className="name">{user.username}</span>
              <span className="divider">·</span>
              <span className={`role role-${user.role.toLowerCase()}`}>{user.role}</span>
            </div>
          </div>

          <button className="zen-logout" onClick={() => void doLogout()} title="退出会话">
            <LogOut size={16} />
          </button>
        </div>
      </header>

      {/* ── 快速任务预设推荐 ────────────────────────────────────────────── */}
      <div className="zen-quick-bar">
        <span className="quick-label">
          <Sparkles size={14} /> 快捷任务推荐:
        </span>
        <div className="quick-pills">
          {missionPresets.map((m) => (
            <button
              key={m.label}
              className="quick-pill"
              onClick={() => handleTriggerRun(m.goal)}
              disabled={running}
            >
              {m.label}
            </button>
          ))}
        </div>
      </div>

      {error && <div className="zen-error-bar">⚠ {error}</div>}

      {/* ── 主空间内容区 ──────────────────────────────────────────────── */}
      <main className="zen-content">
        {space === 'execution' && (
          <ExecutionSpace
            currentRole={user.role}
            runId={runId}
            timeline={timeline}
            events={events}
            running={running}
            onTriggerRun={handleTriggerRun}
          />
        )}

        {space === 'world' && <WorldSpace />}

        {space === 'fabric' && (
          <FabricSpace
            currentUser={user}
            onSwitchUser={(u) => doLogin(u)}
          />
        )}
      </main>

      {/* ── 底部任务执行栏 Composer ─────────────────────────────────────── */}
      <footer className="zen-composer">
        <div className="composer-inner">
          <span className="composer-prompt">❯</span>
          <input
            className="composer-agent"
            value={agentId}
            onChange={(e) => setAgentId(e.target.value)}
            placeholder="智能体 (Agent)"
            title="负责执行的智能体"
          />
          <input
            className="composer-input"
            value={goal}
            onChange={(e) => setGoal(e.target.value)}
            placeholder="输入业务任务目标（例如：为近14天活跃下降的VIP客户制定挽留方案），按 Enter 立即执行..."
            onKeyDown={(e) => e.key === 'Enter' && handleTriggerRun()}
          />
          <button
            className="composer-run"
            onClick={() => handleTriggerRun()}
            disabled={running}
            title="快捷键: Enter"
          >
            {running ? <span className="spin">⏳</span> : <Play size={13} fill="currentColor" />}
            <span>{running ? '执行中…' : '执行任务'}</span>
            <kbd className="composer-kbd">↵</kbd>
          </button>
        </div>
      </footer>
    </div>
  )
}
