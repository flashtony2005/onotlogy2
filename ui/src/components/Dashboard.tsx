import { useEffect, useState } from 'react'
import {
  getEvolution,
  getMemory,
  getPlugins,
  getSessions,
  getSkills,
  getTasks,
} from '../api'
import type {
  EvolutionStateDto,
  MemoryRecord,
  PluginDto,
  SessionSummaryDto,
  SkillDto,
  TaskRecord,
  ViewKind,
} from '../types'

/** 系统总览 Dashboard：指标卡 + 架构视图 + 最近会话 + 快速入口。 */
export function Dashboard({ onNavigate }: { onNavigate: (v: ViewKind) => void }) {
  const [skills, setSkills] = useState<SkillDto[]>([])
  const [plugins, setPlugins] = useState<PluginDto[]>([])
  const [sessions, setSessions] = useState<SessionSummaryDto[]>([])
  const [memories, setMemories] = useState<MemoryRecord[]>([])
  const [tasks, setTasks] = useState<TaskRecord[]>([])
  const [evolution, setEvolution] = useState<EvolutionStateDto | null>(null)
  const [loading, setLoading] = useState(true)
  const [err, setErr] = useState<string | null>(null)

  useEffect(() => {
    ;(async () => {
      setLoading(true)
      setErr(null)
      try {
        const [sk, pl, se, me, ta, ev] = await Promise.all([
          getSkills(),
          getPlugins(),
          getSessions(),
          getMemory(),
          getTasks(),
          getEvolution(),
        ])
        setSkills(sk)
        setPlugins(pl)
        setSessions(se)
        setMemories(me.list)
        setTasks(ta.list)
        setEvolution(ev)
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
  const memHits = memories.reduce((n, m) => n + m.hits, 0)
  const taskPending = tasks.filter((t) => t.status === 'Pending').length
  const taskInprogress = tasks.filter((t) => t.status === 'InProgress').length
  const taskCompleted = tasks.filter((t) => t.status === 'Completed').length
  const patterns = evolution?.patterns.length ?? 0
  const proposals = evolution?.proposals.length ?? 0
  const learned = evolution?.learned.length ?? 0

  return (
    <div className="dash">
      <div className="dash-head">
        <h3>❯ 系统总览</h3>
        <span className="dash-meta">OpenAIP-OS · agent runtime · 实时聚合</span>
      </div>

      {err && <div className="err-msg">⚠️ {err}</div>}
      {loading && <div className="empty-sm">❯ 聚合数据加载中…</div>}

      <div className="dash-grid">
        <div className="dash-card">
          <div className="dash-num">{skills.length}</div>
          <div className="dash-label">Skill Assets</div>
          <div className="dash-sub">{activeSkills} active</div>
        </div>
        <div className="dash-card">
          <div className="dash-num">{plugins.length}</div>
          <div className="dash-label">Plugins</div>
          <div className="dash-sub">{activePlugins} active</div>
        </div>
        <div className="dash-card">
          <div className="dash-num">{sessions.length}</div>
          <div className="dash-label">Sessions</div>
          <div className="dash-sub">{totalEvents} events</div>
        </div>
        <div className="dash-card">
          <div className="dash-num">{memories.length}</div>
          <div className="dash-label">Memory</div>
          <div className="dash-sub">{memHits} hits</div>
        </div>
        <div className="dash-card">
          <div className="dash-num">{patterns}</div>
          <div className="dash-label">Patterns</div>
          <div className="dash-sub">{proposals} proposals · {learned} learned</div>
        </div>
        <div className="dash-card">
          <div className="dash-num">{tasks.length}</div>
          <div className="dash-label">Tasks</div>
          <div className="dash-sub">
            {taskPending} pending · {taskInprogress} running · {taskCompleted} done
          </div>
        </div>
      </div>

      <div className="dash-cols">
        <section className="dash-section">
          <h4>架构分层</h4>
          <div className="arch">
            <div className="arch-row">
              <span className="arch-layer">Skill Layer</span>
              <span className="arch-desc">如何完成目标 — Skill Registry</span>
            </div>
            <div className="arch-row">
              <span className="arch-layer">Capability</span>
              <span className="arch-desc">需要什么能力 — Capability Registry</span>
            </div>
            <div className="arch-row">
              <span className="arch-layer">Plugin Layer</span>
              <span className="arch-desc">谁来提供能力 — Tool / AI / MCP</span>
            </div>
            <div className="arch-row">
              <span className="arch-layer">Runtime</span>
              <span className="arch-desc">Fiber · Policy · Compensation · Audit</span>
            </div>
            <div className="arch-row">
              <span className="arch-layer">Session Log</span>
              <span className="arch-desc">到底发生了什么 — append-only 事实源</span>
            </div>
          </div>
        </section>

        <section className="dash-section">
          <h4>最近 Sessions</h4>
          {sessions.length === 0 ? (
            <div className="empty-sm">暂无 Session — 前往 V2 Runtime 创建</div>
          ) : (
            <ul className="dash-sessions">
              {sessions.slice(0, 6).map((s) => (
                <li key={s.session_id}>
                  <code>{s.session_id}</code>
                  <span>{s.events} events</span>
                </li>
              ))}
            </ul>
          )}
        </section>
      </div>

      <section className="dash-section">
        <h4>快速入口</h4>
        <div className="dash-quick">
          <button onClick={() => onNavigate('v2')}>▣ V2 Runtime — Skill / Plugin / Session</button>
          <button onClick={() => onNavigate('evolution')}>◉ Evolution — 自学习知识</button>
          <button onClick={() => onNavigate('memory')}>💾 Memory — 动作沉淀</button>
          <button onClick={() => onNavigate('tasks')}>☑ Tasks — 任务状态</button>
          <button onClick={() => onNavigate('developer')}>◈ Developer — 全量事件流</button>
        </div>
      </section>
    </div>
  )
}
