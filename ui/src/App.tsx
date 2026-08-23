import { useEffect, useMemo, useState } from 'react'
import {
  Activity,
  Boxes,
  ChevronDown,
  ChevronRight,
  BookOpen,
  Code2,
  Globe2,
  FolderKanban,
  LayoutDashboard,
  LogOut,
  Network,
  PackageOpen,
  ShieldCheck,
  Sparkles,
  Users,
  Wifi,
  WifiOff,
} from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import {
  AuthError,
  getMe,
  postLogin,
  postLogout,
  postRun,
} from './api'
import { LoginForm } from './components/LoginForm'
import { LoginScene } from './components/LoginScene'
import { V2Panel } from './components/V2Panel'
import { Dashboard } from './components/Dashboard'
import { FabricPanel } from './components/FabricPanel'
import { ProjectPanel } from './components/ProjectPanel'
import { UserManagementPanel } from './components/UserManagementPanel'
import { BusinessPackPanel } from './components/BusinessPackPanel'
import { GovernancePanel } from './components/GovernancePanel'
import { KnowledgePanel } from './components/KnowledgePanel'
import { WorldPanel } from './components/WorldPanel'
import { fmtJson, fmtTime } from './format'
import { clearToken, getToken, setToken } from './auth'
import { useTraceStream } from './hooks/useTraceStream'
import { buildTimeline } from './view'
import type { AuthUserDto, TraceEventDto, ViewKind } from './types'

const VIEW_GROUPS: { name: string; en: string; items: ViewKind[] }[] = [
  { name: '运行', en: 'Operations', items: ['overview', 'world', 'runs'] },
  { name: '学习', en: 'Learning', items: ['workers', 'learning'] },
  { name: '治理', en: 'Governance', items: ['projects', 'knowledge', 'business-packs', 'skills', 'governance', 'fabric', 'developer', 'user-management'] },
]

interface ViewMeta {
  key: ViewKind
  label: string
  cn: string
  q: string
  icon: LucideIcon
  desc: string
}

const VIEWS: ViewMeta[] = [
  {
    key: 'overview',
    label: 'Overview',
    cn: '总览',
    icon: LayoutDashboard,
    q: '现在系统怎么样？',
    desc: '整个系统当前状态、运行情况、重要指标、有没有异常',
  },
  {
    key: 'world',
    label: 'World',
    cn: '世界',
    icon: Globe2,
    q: '世界现在是什么状态？',
    desc: '世界对象、关系图、对象当前状态与变化链',
  },
  {
    key: 'runs',
    label: 'Runs',
    cn: '运行',
    icon: Activity,
    q: 'Agent 正在做什么？',
    desc: '运行中的 Agent、任务状态、认知闭环推进到哪一步',
  },
  {
    key: 'workers',
    label: 'Workers',
    cn: '工人',
    icon: Users,
    q: '谁在执行？',
    desc: 'Worker 能力、状态、质量与执行记录',
  },
  {
    key: 'learning',
    label: 'Learning',
    cn: '学习',
    icon: Sparkles,
    q: '系统学到了什么？',
    desc: '经验沉淀、Pattern、Proposal、Learned 知识',
  },
  {
    key: 'governance',
    label: 'Governance',
    cn: '治理',
    icon: ShieldCheck,
    q: '有没有越界？',
    desc: '决策摘要、策略检查、审批与审计',
  },
  {
    key: 'fabric',
    label: 'Fabric',
    cn: '通信',
    icon: Network,
    q: 'Agent 之间在说什么？',
    desc: 'Agent 拓扑、通信事件流、任务提交（OAB）',
  },
  {
    key: 'projects',
    label: 'Projects',
    cn: '项目',
    icon: FolderKanban,
    q: '现在管理哪些项目？',
    desc: '项目列表、创建、工程本体 seed、项目世界与审计（Phase 8）',
  },
  {
    key: 'knowledge',
    label: 'Knowledge',
    cn: '知识推理',
    icon: BookOpen,
    q: '本体当前是否一致？',
    desc: '查看不可变本体版本、Runtime 投影与 TBox/ABox 推理诊断',
  },
  {
    key: 'developer',
    label: 'Developer',
    cn: '开发者',
    icon: Code2,
    q: '到底发生了什么？',
    desc: '输入、输出、API、Tool 调用、事件、错误、完整技术日志',
  },
  {
    key: 'business-packs',
    label: 'Packs',
    cn: '业务包',
    icon: PackageOpen,
    q: '管理业务本体与操作 Skill？',
    desc: '扫描、校验、安装业务包，查看本体与 Skill 关联',
  },
  {
    key: 'skills',
    label: 'Skills',
    cn: 'Skill 管理',
    icon: Sparkles,
    q: '系统如何完成目标？',
    desc: '查看、导入和追溯 Skill 原始内容',
  },
  {
    key: 'user-management',
    label: 'Users',
    cn: '用户管理',
    icon: Users,
    q: '管理谁可以进入系统？',
    desc: '创建用户、重设密码、启用/禁用账号与角色',
  },
]

export default function App() {
  const [view, setView] = useState<ViewKind>('overview')
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({})
  const [agentId, setAgentId] = useState('retention-agent')
  const [goal, setGoal] = useState('为高风险流失客户生成挽留计划')
  const [runId, setRunId] = useState<string | null>(null)
  const [events, setEvents] = useState<TraceEventDto[]>([])
  const [connected, setConnected] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [running, setRunning] = useState(false)

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

  /** 切换侧栏分组展开/收起。 */
  const toggleGroup = (name: string) => {
    setCollapsed((prev) => ({ ...prev, [name]: !prev[name] }))
  }

  /** 跳转视图并自动展开所属分组（总览快捷入口也可直达折叠中的菜单）。 */
  const goView = (k: ViewKind) => {
    setView(k)
    const g = VIEW_GROUPS.find((x) => x.items.includes(k))
    if (g) setCollapsed((prev) => (prev[g.name] ? { ...prev, [g.name]: false } : prev))
    if (k === 'learning') {
    }
  }

  // Memory 视图刷新：从 REST /memory 拉取沉淀（hits 随轮次累加）。

  // 节流版刷新（useMemo 保持单例；内部只依赖稳定 setState，捕获旧闭包无碍）

  // WS 订阅：有 run 时跟 run 流；无 run 时连 agent 流（登录即连接，run 启动后事件实时到达）
  useTraceStream(runId ?? agentId, {
    enabled: !!user,
    onEvent: (e) => {
      setEvents((prev) => (prev.some((p) => p.id === e.id) ? prev : [...prev, e]))
      // 记忆沉淀/召回事件 → 实时刷新 Memory 视图（hits 累加可见）
      // U4：事件高频到达时合并刷新（300ms 窗口一次），避免并发 REST 互相覆盖

      if (e.eventType === 'Action' || e.eventType === 'Planning') {
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
    } catch (e) {
      if (!handleAuthError(e)) setError((e as Error).message)
    } finally {
      setRunning(false)
    }
  }

  // 未登录：渲染治愈插画风登录页（V0.7 认证强制）
  if (sessionChecking) {
    return (
      <div className="auth-page">
        <div className="auth-empty">❯ 正在恢复会话…</div>
      </div>
    )
  }

  if (!user) {
    return (
      <div className="auth-page">
        {/* 顶部导航 */}
        <nav className="auth-nav">
          <div className="auth-brand">
            <span className="auth-brand-leaf">🌿</span> OpenAIP-OS
          </div>
          <div className="auth-nav-links">
            <a href="#" onClick={(e) => e.preventDefault()}>功能</a>
            <a href="#" onClick={(e) => e.preventDefault()}>文档</a>
            <a href="#" onClick={(e) => e.preventDefault()}>关于</a>
            <button className="auth-nav-btn" type="button">LOGIN</button>
          </div>
        </nav>

        {/* 山水插画背景 */}
        <div className="auth-scene">
          <LoginScene />
        </div>

        {/* 悬浮登录卡片 */}
        <div className="auth-form-wrap">
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
          <span className="side-logo">
            <Boxes size={18} strokeWidth={2.2} />
          </span>
          <div className="side-brand-text">
            <span className="side-title">OpenAIP-OS</span>
            <span className="side-subtitle">Ontology Agent Runtime</span>
          </div>
        </div>

        <nav className="side-nav">
          {VIEW_GROUPS.map((g) => (
            <div key={g.name} className="side-group">
              <button
                className="side-group-name"
                onClick={() => toggleGroup(g.name)}
                title={collapsed[g.name] ? `展开 ${g.name}` : `收起 ${g.name}`}
              >
                <span>{g.name}</span>
                <span className="side-group-en">{g.en}</span>
                <span className="side-group-caret">
                  {collapsed[g.name] ? <ChevronRight size={13} /> : <ChevronDown size={13} />}
                </span>
              </button>
              {!collapsed[g.name] && g.items.filter((k) => k !== 'user-management' || user.role === 'developer').map((k) => {
                const v = VIEWS.find((x) => x.key === k)!
                return (
                  <button
                    key={k}
                    className={view === k ? 'side-btn active' : 'side-btn'}
                    onClick={() => goView(k)}
                    title={v.desc}
                  >
                    <span className="side-ic">
                      <v.icon size={16} strokeWidth={2} />
                    </span>
                    <span className="side-text">
                      <span className="side-label">{v.cn}</span>
                      <span className="side-desc">{v.q}</span>
                    </span>
                  </button>
                )
              })}
            </div>
          ))}
        </nav>

        <div className="side-bottom">
          <div className="side-user">
            <span className="side-avatar">{user.username.slice(0, 1).toUpperCase()}</span>
            <div className="side-user-meta">
              <span className="side-user-name">{user.username}</span>
              <span className={`role role-${user.role.toLowerCase()}`}>{user.role}</span>
            </div>
          </div>
          <div className={`side-conn ${connected ? 'on' : 'off'}`}>
            <span className="dot" />
            <span>{connected ? 'WS 已连接' : 'WS 未连接'}</span>
          </div>
          <button className="side-btn side-logout" onClick={() => void doLogout()}>
            <span className="side-ic">
              <LogOut size={16} strokeWidth={2} />
            </span>
            <span className="side-text">
              <span className="side-label">登出</span>
            </span>
          </button>
        </div>
      </aside>

      <div className="main-col">
        {/* 顶部 mini bar：会话上下文 + 连接状态 */}
        <header className="mini-bar">
          <div className="mini-title">
            <span className="mini-logo">
              <Boxes size={17} strokeWidth={2.2} />
            </span>
            <span className="mini-name">OpenAIP-OS</span>
            <span className="mini-sep">/</span>
            {(() => {
              const v = VIEWS.find((x) => x.key === view)
              return (
                <span className="mini-sub">{v ? `${v.cn} · ${v.desc}` : ''}</span>
              )
            })()}
          </div>
          <div className="conn-group">
            <div className={`conn ${connected ? 'on' : 'off'}`}>
              {connected ? <Wifi size={13} /> : <WifiOff size={13} />}
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
        {!runId && (view === 'runs' || view === 'developer') && (
          <div className="empty">
            输入目标并点击「触发 Agent 运行」，实时观察 Agent 认知闭环。
          </div>
        )}

        {view === 'overview' && (
          <Dashboard
            onNavigate={goView}
            events={events}
            runId={runId}
            runStatus={runId ? timeline.status : null}
          />
        )}

        {view === 'world' && <WorldPanel />}
        {view === 'knowledge' && <KnowledgePanel />}
        {view === 'fabric' && <FabricPanel />}
        {view === 'projects' && <ProjectPanel />}
        {view === 'business-packs' && (
          <BusinessPackPanel currentRole={user.role} onAuthError={handleAuthError} />
        )}
        {view === 'skills' && <V2Panel initialTab="skills" />}
        {view === 'user-management' && user.role === 'developer' && (
          <UserManagementPanel currentUsername={user.username} onAuthError={handleAuthError} />
        )}

        {view === 'workers' && <V2Panel initialTab="workers" />}

        {view === 'learning' && <V2Panel initialTab="workers" />}

        {view === 'governance' && <GovernancePanel />}

        {view === 'runs' && (
          <div className="empty">run 时间线见上方（V2 事件流）。</div>
        )}

        {runId && timeline.decision && view === 'governance' && (
          <div className="decision">
            <h3>决策摘要</h3>
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

        {runId && (view === 'runs' || view === 'developer') && timeline.events.length === 0 && (
          <div className="empty">等待认知事件…</div>
        )}

        {(view === 'runs' || view === 'developer') && timeline.events.length > 0 &&
          timeline.events.map((ev) => (
          <article key={ev.id} className={`event ${ev.eventType.toLowerCase()}`}>
            <div className="ev-head">
              <span className="ic">•</span>
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
