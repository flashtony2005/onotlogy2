import { useEffect, useRef, useState } from 'react'
import {
  Boxes,
  LayoutGrid,
  PlayCircle,
  Plug,
  Route,
} from 'lucide-react'
import { getPlugins, getSessions, getSkills } from '../api'
import type {
  PluginDto,
  SessionSummaryDto,
  SkillDto,
  TraceEventDto,
  ViewKind,
} from '../types'

/** 数字滚动动画（0 → target，ease-out）。 */
function useCountUp(target: number, durationMs = 900): number {
  const [value, setValue] = useState(0)
  const raf = useRef<number | null>(null)
  useEffect(() => {
    const start = performance.now()
    const tick = (t: number) => {
      const p = Math.min(1, (t - start) / durationMs)
      setValue(Math.round(target * (1 - Math.pow(1 - p, 3))))
      if (p < 1) raf.current = requestAnimationFrame(tick)
    }
    raf.current = requestAnimationFrame(tick)
    return () => {
      if (raf.current) cancelAnimationFrame(raf.current)
    }
  }, [target, durationMs])
  return value
}

/** 横向条形图（加载后动画生长）。 */
function Bar({ label, value, max }: { label: string; value: number; max: number }) {
  const pct = max > 0 ? Math.min(100, (value / max) * 100) : 0
  return (
    <div className="dash-bar">
      <span className="dash-bar-label">{label}</span>
      <div className="dash-bar-track">
        <div className="dash-bar-fill" style={{ width: `${pct}%` }} />
      </div>
      <span className="dash-bar-val">{value}</span>
    </div>
  )
}

/** 环形图（SVG stroke-dasharray 分段）。 */
function Donut({
  segments,
  center,
  centerLabel,
}: {
  segments: { label: string; value: number; color: string }[]
  center: number
  centerLabel: string
}) {
  const total = segments.reduce((n, s) => n + s.value, 0) || 1
  const R = 42
  const C = 2 * Math.PI * R
  let acc = 0
  return (
    <div className="dash-donut">
      <svg viewBox="0 0 120 120" className="dash-donut-svg">
        <circle cx="60" cy="60" r={R} fill="none" stroke="var(--border-soft)" strokeWidth="12" />
        {segments.map((s) => {
          const frac = s.value / total
          const dash = frac * C
          const offset = -acc * C
          acc += frac
          return (
            <circle
              key={s.label}
              cx="60"
              cy="60"
              r={R}
              fill="none"
              stroke={s.color}
              strokeWidth="12"
              strokeDasharray={`${dash} ${C - dash}`}
              strokeDashoffset={offset}
              transform="rotate(-90 60 60)"
              className="dash-donut-seg"
            >
              <title>{`${s.label}: ${s.value}`}</title>
            </circle>
          )
        })}
      </svg>
      <div className="dash-donut-center">
        <span className="dash-donut-num">{center}</span>
        <span className="dash-donut-label">{centerLabel}</span>
      </div>
    </div>
  )
}

/** 架构流动图（5 层 + 流动光点动画）。 */
function ArchFlow() {
  const layers = [
    { name: 'SKILL LAYER', desc: '如何完成目标 · Skill Registry', icon: '▣' },
    { name: 'CAPABILITY', desc: '需要什么能力 · Capability Registry', icon: '◆' },
    { name: 'PLUGIN LAYER', desc: '谁来提供能力 · Tool / AI / MCP', icon: '🔌' },
    { name: 'RUNTIME', desc: 'Fiber · Policy · Compensation · Audit', icon: '⚙' },
    { name: 'SESSION LOG', desc: '到底发生了什么 · append-only', icon: '📜' },
  ]
  return (
    <div className="arch-flow">
      {layers.map((l, i) => (
        <div key={l.name} className="arch-flow-step">
          <div className="arch-flow-node">
            <span className="arch-flow-ic">{l.icon}</span>
            <div>
              <div className="arch-flow-name">{l.name}</div>
              <div className="arch-flow-desc">{l.desc}</div>
            </div>
          </div>
          {i < layers.length - 1 && (
            <div className="arch-flow-line">
              <span className="arch-flow-dot" style={{ animationDelay: `${i * 0.3}s` }} />
            </div>
          )}
        </div>
      ))}
    </div>
  )
}

const LOOP_STEPS = [
  { label: 'Goal', eventType: 'Goal' },
  { label: 'Think', eventType: 'Reasoning' },
  { label: 'Govern', eventType: 'PolicyCheck' },
  { label: 'Worker', eventType: 'Planning' },
  { label: 'Action', eventType: 'Action' },
  { label: 'Event', eventType: 'MemoryUpdate' },
  { label: 'World', eventType: 'World' },
]

/** Live Agent Loop：根据实时事件推导每一步的 done/active/todo。 */
function LoopStrip({
  events,
  runId,
  runStatus,
}: {
  events: TraceEventDto[]
  runId: string | null
  runStatus: string | null
}) {
  const seen = new Set(events.map((e) => e.eventType))
  const worldSeen = events.some((e) => e.objectRefs && e.objectRefs.length > 0)
  const firstUndone = LOOP_STEPS.findIndex((s) => (s.eventType === 'World' ? !worldSeen : !seen.has(s.eventType)))
  return (
    <div className="loop">
      <div className="loop-head">
        <h4>Live Agent Loop</h4>
        <span className={`loop-pill ${runId ? (runStatus ?? '').toLowerCase() : 'idle'}`}>
          {runId ? runStatus ?? 'RUNNING' : 'IDLE'}
        </span>
      </div>
      <div className="loop-strip">
        {LOOP_STEPS.map((s, i) => {
          const done = s.eventType === 'World' ? worldSeen : seen.has(s.eventType)
          const active = !!runId && i === firstUndone
          return (
            <div key={s.label} className="loop-item">
              <span className={`loop-step ${done ? 'done' : active ? 'active' : runId ? 'todo' : 'idle'}`}>
                {s.label}
              </span>
              {i < LOOP_STEPS.length - 1 && <span className="loop-arrow">→</span>}
            </div>
          )
        })}
      </div>
    </div>
  )
}

/** 系统总览 Dashboard：环形图 + 维度条形 + 指标卡 + 架构流动 + 快速入口。 */
export function Dashboard({
  onNavigate,
  events = [],
  runId = null,
  runStatus = null,
}: {
  onNavigate: (v: ViewKind) => void
  events?: TraceEventDto[]
  runId?: string | null
  runStatus?: string | null
}) {
  const [skills, setSkills] = useState<SkillDto[]>([])
  const [plugins, setPlugins] = useState<PluginDto[]>([])
  const [sessions, setSessions] = useState<SessionSummaryDto[]>([])
  const [loading, setLoading] = useState(true)
  const [err, setErr] = useState<string | null>(null)

  useEffect(() => {
    ;(async () => {
      setLoading(true)
      setErr(null)
      try {
        const [sk, pl, se] = await Promise.all([
          getSkills(),
          getPlugins(),
          getSessions(),
        ])
        setSkills(sk)
        setPlugins(pl)
        setSessions(se)
      } catch (e) {
        setErr(String(e))
      } finally {
        setLoading(false)
      }
    })()
  }, [])

  const activeSkills = skills.filter((s) => s.status === 'Active').length
  const activePlugins = plugins.filter((p) => p.status === 'Active').length
  const totalEvents = sessions.reduce((n, s) => n + s.events, 0)

  // 动画数字
  const nSkills = useCountUp(skills.length)
  const nPlugins = useCountUp(plugins.length)
  const nSessions = useCountUp(sessions.length)
  const nEvents = useCountUp(totalEvents)

  // Skill 类型分布（donut 段）
  const typeCount = (t: string) => skills.filter((s) => s.skill_type === t).length
  const typeSegs = [
    { label: 'Composite', value: typeCount('Composite'), color: '#4c9ee6' },
    { label: 'Workflow', value: typeCount('Workflow'), color: '#3fae7f' },
    { label: 'Atomic', value: typeCount('Atomic'), color: '#9f8fd6' },
    {
      label: 'Other',
      value: Math.max(
        0,
        skills.length - typeCount('Composite') - typeCount('Workflow') - typeCount('Atomic'),
      ),
      color: '#b3c6bd',
    },
  ].filter((s) => s.value > 0)

  // 六维条形
  const dimMax = Math.max(skills.length, plugins.length, sessions.length, 1)
  const dims = [
    { label: 'Skill', value: skills.length },
    { label: 'Plugin', value: plugins.length },
    { label: 'Session', value: sessions.length },
  ]

  const now = new Date().toLocaleTimeString('zh-CN', { hour12: false })

  const objects = [
    { label: 'Skill', count: skills.length, icon: Boxes, color: 'var(--accent)' },
    { label: 'Plugin', count: plugins.length, icon: Plug, color: 'var(--purple)' },
    { label: 'Session', count: sessions.length, icon: Route, color: 'var(--teal)' },
  ]

  return (
    <div className="dash">
      <div className="dash-head">
        <div>
          <h3>运营总览</h3>
          <span className="dash-meta">OpenAIP-OS · Agent Runtime · {now}</span>
        </div>
        <span className="dash-pill">实时</span>
      </div>

      {err && <div className="err-msg">⚠️ {err}</div>}
      {loading && <div className="empty-sm">❯ 聚合数据加载中…</div>}

      <LoopStrip events={events} runId={runId} runStatus={runStatus} />

      {/* 核心区：环形图（Skill 分布）+ 六维条形 */}
      <div className="dash-hero">
        <div className="dash-hero-donut">
          <Donut
            segments={
              typeSegs.length > 0 ? typeSegs : [{ label: 'Empty', value: 1, color: '#e2efe8' }]
            }
            center={nSkills}
            centerLabel="SKILLS"
          />
          <div className="dash-donut-legend">
            {typeSegs.map((s) => (
              <span key={s.label} className="dash-legend-item">
                <i style={{ background: s.color }} />
                {s.label} <b>{s.value}</b>
              </span>
            ))}
          </div>
        </div>
        <div className="dash-hero-bars">
          <h4>能力维度分布</h4>
          {dims.map((d) => (
            <Bar key={d.label} label={d.label} value={d.value} max={dimMax} />
          ))}
        </div>
      </div>

      {/* 对象类型网格 */}
      <section className="dash-section">
        <h4>
          <LayoutGrid size={14} /> 对象类型
        </h4>
        <div className="obj-grid">
          {objects.map((o) => (
            <button
              key={o.label}
              className="obj-card"
              onClick={() => {
                onNavigate('v2')
              }}
              title={`查看 ${o.label}`}
            >
              <span className="obj-ic" style={{ color: o.color, background: `color-mix(in srgb, ${o.color} 12%, white)` }}>
                <o.icon size={18} strokeWidth={2} />
              </span>
              <span className="obj-count">{o.count}</span>
              <span className="obj-label">{o.label}</span>
            </button>
          ))}
        </div>
      </section>

      {/* 指标卡行（数字滚动） */}
      <div className="dash-grid">
        <div className="dash-card">
          <div className="dash-num">{nPlugins}</div>
          <div className="dash-label">Plugins</div>
          <div className="dash-sub">{activePlugins} active</div>
        </div>
        <div className="dash-card">
          <div className="dash-num">{nSessions}</div>
          <div className="dash-label">Sessions</div>
          <div className="dash-sub">{nEvents} events</div>
        </div>
        <div className="dash-card">
          <div className="dash-num">{activeSkills}</div>
          <div className="dash-label">Active Skills</div>
          <div className="dash-sub">of {skills.length} registered</div>
        </div>
      </div>

      {/* 架构流动图 */}
      <section className="dash-section">
        <h4>架构分层 · 请求流动</h4>
        <ArchFlow />
      </section>

      <section className="dash-section">
        <h4>快速入口</h4>
        <div className="dash-quick">
          <button onClick={() => onNavigate('v2')}>
            <Boxes size={15} /> V2 Runtime
          </button>
          <button onClick={() => onNavigate('developer')}>
            <PlayCircle size={15} /> 全量事件流
          </button>
        </div>
      </section>
    </div>
  )
}
