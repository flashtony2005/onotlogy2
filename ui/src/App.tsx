import { useState, useEffect, useMemo } from 'react'
import {
  Boxes,
  Target,
  Globe2,
  Cpu,
  LogOut,
  Sparkles,
  Play,
  BookOpen,
  X,
  ExternalLink,
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
  const [agentId, setAgentId] = useState('global-supply-agent')
  const [goal, setGoal] = useState('为美西长滩海外仓高危断货爆款 SKU 制定跨仓空运平衡方案')
  const [runId, setRunId] = useState<string | null>(null)
  const [events, setEvents] = useState<TraceEventDto[]>([])
  const [connected, setConnected] = useState(true)
  const [running, setRunning] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [showManualModal, setShowManualModal] = useState(false)

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
    {
      label: '📦 黑五断货紧急调拨',
      goal: '对近3天销量激增、美西海外仓库存跌破安全水位的爆款 SKU 启动跨仓空运平衡',
    },
    {
      label: '💶 欧洲多站点 VAT 汇率测算',
      goal: '检测欧元贬值与欧盟新关税下全品类毛利不变式与调价补贴区间',
    },
    {
      label: '🚢 全球海运提单与多仓体检',
      goal: '全面体检美西、美东、汉堡仓全链路库存健康度与周转天数 (DSI)',
    },
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
            <span className="auth-brand-leaf">🌿</span> AuraGlobal · 跨境出海智能业务协同工作台
          </div>
          <div className="auth-nav-links">
            <button
              onClick={() => setShowManualModal(true)}
              style={{
                background: '#f0fdf4',
                border: '1px solid #bbf7d0',
                color: '#166534',
                padding: '6px 12px',
                borderRadius: '8px',
                cursor: 'pointer',
                display: 'inline-flex',
                alignItems: 'center',
                gap: '6px',
                fontSize: '12px',
                fontWeight: 600,
              }}
            >
              <BookOpen size={14} /> 📘 查看出海本体建模操作手册 (带图)
            </button>
            <span style={{ fontSize: '12px', color: '#6d857b' }}>简单明了 · 高效协同</span>
          </div>
        </nav>

        <div className="auth-scene">
          <LoginScene />
        </div>

        <div className="auth-form-wrap">
          <LoginForm error={loginError} onLogin={doLogin} />
        </div>

        {/* 独立手册浮层（登录前亦可直接查阅） */}
        {showManualModal && (
          <div className="manual-modal-backdrop" onClick={() => setShowManualModal(false)}>
            <div className="manual-modal-card" onClick={(e) => e.stopPropagation()}>
              <div className="manual-modal-header">
                <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                  <BookOpen size={18} color="#0f766e" />
                  <h3 style={{ margin: 0, fontSize: '16px', color: '#0f172a' }}>
                    AuraGlobal 出海网站业务分析与本体论建模操作手册 (带矢量图解)
                  </h3>
                </div>
                <div style={{ display: 'flex', alignItems: 'center', gap: '10px' }}>
                  <a
                    href="/manual.html"
                    target="_blank"
                    rel="noreferrer"
                    className="btn-manual-external"
                    title="新标签页打开独立打印版本"
                  >
                    <ExternalLink size={14} /> 独立窗口打开
                  </a>
                  <button className="manual-close-btn" onClick={() => setShowManualModal(false)}>
                    <X size={18} />
                  </button>
                </div>
              </div>
              <div className="manual-modal-body">
                <iframe src="/manual.html" title="操作手册" className="manual-iframe" />
              </div>
            </div>
          </div>
        )}
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
            <span className="zen-title">AuraGlobal-OS</span>
            <span className="zen-sub">跨境出海智能协同平台</span>
          </div>
        </div>

        {/* 核心工作空间选择器 */}
        <nav className="zen-spaces">
          <button
            className={`space-nav-btn ${space === 'execution' ? 'active' : ''}`}
            onClick={() => setSpace('execution')}
          >
            <Target size={16} />
            <span>业务大盘</span>
            <span className="sub-tag">Dashboard</span>
          </button>

          <button
            className={`space-nav-btn ${space === 'world' ? 'active' : ''}`}
            onClick={() => setSpace('world')}
          >
            <Globe2 size={16} />
            <span>业务图谱</span>
            <span className="sub-tag">Graph</span>
          </button>

          <button
            className={`space-nav-btn ${space === 'fabric' ? 'active' : ''}`}
            onClick={() => setSpace('fabric')}
          >
            <Cpu size={16} />
            <span>智能团队</span>
            <span className="sub-tag">Agents</span>
          </button>
        </nav>

        {/* 操作手册按钮与用户状态 */}
        <div className="zen-user-group">
          <button
            className="zen-manual-btn"
            onClick={() => setShowManualModal(true)}
            title="阅读出海业务本体论建模操作手册（含全流程架构图）"
          >
            <BookOpen size={15} />
            <span>📘 出海本体手册 (带图)</span>
          </button>

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
          <Sparkles size={14} /> 跨境任务推荐:
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
            title="负责执行的出海智能体"
          />
          <input
            className="composer-input"
            value={goal}
            onChange={(e) => setGoal(e.target.value)}
            placeholder="输入出海业务任务目标（例如：为美西长滩海外仓高危断货爆款 SKU 制定跨仓空运平衡方案），按 Enter 立即执行..."
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

      {/* 沉浸式出海本体操作手册视窗 */}
      {showManualModal && (
        <div className="manual-modal-backdrop" onClick={() => setShowManualModal(false)}>
          <div className="manual-modal-card" onClick={(e) => e.stopPropagation()}>
            <div className="manual-modal-header">
              <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                <BookOpen size={18} color="#0f766e" />
                <h3 style={{ margin: 0, fontSize: '16px', color: '#0f172a' }}>
                  AuraGlobal 出海网站业务分析与本体论建模操作手册 (带矢量图解)
                </h3>
              </div>
              <div style={{ display: 'flex', alignItems: 'center', gap: '10px' }}>
                <a
                  href="/manual.html"
                  target="_blank"
                  rel="noreferrer"
                  className="btn-manual-external"
                  title="新标签页打开独立打印版本"
                >
                  <ExternalLink size={14} /> 独立窗口打开
                </a>
                <button className="manual-close-btn" onClick={() => setShowManualModal(false)}>
                  <X size={18} />
                </button>
              </div>
            </div>
            <div className="manual-modal-body">
              <iframe src="/manual.html" title="操作手册" className="manual-iframe" />
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
