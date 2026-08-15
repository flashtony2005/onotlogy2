import { useEffect, useState } from 'react'
import {
  createSession,
  getPlugins,
  getSession,
  getSkillRaw,
  getSkills,
  importSkill,
} from '../api'
import type { PluginDto, RawSkillDto, SessionDto, SkillDto } from '../types'

const DEMO_SKILL_MD = `# YouTube Channel Growth

帮助 YouTube 频道实现从利基研究到变现优化的完整增长闭环。

- goal: 通过系统化的内容策略实现 YouTube 频道增长

## Stage 1: Niche Research
- capability: web.search
- artifact: NicheReport

## Stage 2: Video Production
- capability: video.generate
- artifact: VideoFile

### Sub-skill: NicheResearch
### Sub-skill: VideoProduction

- metric: CTR
- feedback: AnalyticsOptimization -> ContentStrategy
`

const KNOWN = ['web.search', 'video.generate', 'llm.reason']

/** Phase 10 V2 认知面板：Skill Registry / Plugin Registry / Session Log。 */
export function V2Panel() {
  const [tab, setTab] = useState<'skills' | 'plugins' | 'sessions'>('skills')
  const [skills, setSkills] = useState<SkillDto[]>([])
  const [plugins, setPlugins] = useState<PluginDto[]>([])
  const [session, setSession] = useState<SessionDto | null>(null)
  const [loading, setLoading] = useState(false)
  const [msg, setMsg] = useState<string | null>(null)
  const [err, setErr] = useState<string | null>(null)
  // 上传表单
  const [upName, setUpName] = useState('my-skill')
  const [upContent, setUpContent] = useState('')
  const [upKnown, setUpKnown] = useState('')
  const [upOpen, setUpOpen] = useState(false)
  // 原始内容展开（skill id → raw）
  const [raws, setRaws] = useState<Record<string, RawSkillDto>>({})
  const [rawLoading, setRawLoading] = useState<string | null>(null)

  const reloadSkills = async () => {
    setLoading(true)
    try {
      setSkills(await getSkills())
      setErr(null)
    } catch (e) {
      setErr(String(e))
    } finally {
      setLoading(false)
    }
  }
  const reloadPlugins = async () => {
    setLoading(true)
    try {
      setPlugins(await getPlugins())
      setErr(null)
    } catch (e) {
      setErr(String(e))
    } finally {
      setLoading(false)
    }
  }
  const reloadSession = async (id: string) => {
    setLoading(true)
    try {
      setSession(await getSession(id))
      setErr(null)
    } catch (e) {
      setErr(String(e))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    void reloadSkills()
  }, [])

  const onImport = async () => {
    setMsg(null)
    setErr(null)
    try {
      const s = await importSkill('youtube-channel-growth', DEMO_SKILL_MD, KNOWN)
      setMsg(`✅ 已导入 ${s.id}（${s.skill_type}，${s.capabilities.length} 个能力）`)
      await reloadSkills()
    } catch (e) {
      setErr(String(e))
    }
  }
  const onToggleRaw = async (id: string) => {
    if (raws[id]) {
      setRaws((r) => {
        const next = { ...r }
        delete next[id]
        return next
      })
      return
    }
    setRawLoading(id)
    setErr(null)
    try {
      const raw = await getSkillRaw(id)
      setRaws((r) => ({ ...r, [id]: raw }))
    } catch (e) {
      setErr(String(e))
    } finally {
      setRawLoading(null)
    }
  }

  const onUpload = async () => {
    if (!upContent.trim()) {
      setErr('请粘贴 SKILL.md 原始内容')
      return
    }
    setMsg(null)
    setErr(null)
    try {
      const known = upKnown
        .split(/[,，]/)
        .map((x) => x.trim())
        .filter(Boolean)
      const s = await importSkill(upName.trim() || 'my-skill', upContent, known)
      setMsg(`✅ 已上传 ${s.id}（${s.skill_type}，${s.capabilities.length} 个能力解析）`)
      setUpContent('')
      setUpOpen(false)
      await reloadSkills()
    } catch (e) {
      setErr(String(e))
    }
  }

  const onNewSession = async () => {
    setMsg(null)
    setErr(null)
    try {
      const id = `swe-${Date.now().toString(36)}`
      setSession(await createSession(id))
      setMsg(`✅ Session ${id} 已创建`)
    } catch (e) {
      setErr(String(e))
    }
  }

  return (
    <div className="v2">
      <div className="v2-tabs">
        {(['skills', 'plugins', 'sessions'] as const).map((t) => (
          <button
            key={t}
            className={tab === t ? 'v2-tab active' : 'v2-tab'}
            onClick={() => {
              setTab(t)
              if (t === 'plugins') void reloadPlugins()
            }}
          >
            {t === 'skills' ? '🛠 Skill Registry' : t === 'plugins' ? '🔌 Plugin Registry' : '📜 Session Log'}
          </button>
        ))}
      </div>

      {msg && <div className="ok-msg">{msg}</div>}
      {err && <div className="err-msg">⚠️ {err}</div>}

      {tab === 'skills' && (
        <div className="v2-section">
          <div className="v2-head">
            <h3>Skill Registry —「如何完成目标」</h3>
            <div>
              <button className="reload" onClick={() => void reloadSkills()} disabled={loading}>
                {loading ? '刷新中…' : '↻ 刷新'}
              </button>
              <button className="reload" onClick={onImport} disabled={loading}>
                ⬇ YouTube 示例
              </button>
              <button className="reload" onClick={() => setUpOpen((o) => !o)}>
                {upOpen ? '✕ 收起上传' : '⬆ 上传 SKILL.md'}
              </button>
            </div>
          </div>

          {upOpen && (
            <div className="up-form">
              <div className="up-row">
                <input
                  className="up-name"
                  value={upName}
                  onChange={(e) => setUpName(e.target.value)}
                  placeholder="skill id"
                />
                <input
                  className="up-known"
                  value={upKnown}
                  onChange={(e) => setUpKnown(e.target.value)}
                  placeholder="已知能力（逗号分隔，可选）"
                />
              </div>
              <textarea
                className="up-textarea"
                value={upContent}
                onChange={(e) => setUpContent(e.target.value)}
                placeholder={'# My Skill\n\n- goal: ...\n## Stage 1: ...\n- capability: ...'}
                rows={7}
              />
              <div className="up-actions">
                <button className="run up-run" onClick={onUpload}>
                  ⬆ 解析并注册
                </button>
                <span className="up-hint">
                  原始内容永久保留（Raw 不修改），解析后可在列表展开查看
                </span>
              </div>
            </div>
          )}

          {skills.length === 0 ? (
            <div className="empty">暂无 Skill。上传 SKILL.md 或导入示例。</div>
          ) : (
            <ul className="v2-list">
              {skills.map((s) => (
                <li key={s.id} className="v2-item">
                  <div className="v2-item-head">
                    <code>{s.id}</code>
                    <span className="badge">{s.skill_type}</span>
                    <span className="badge dim">v{s.version}</span>
                    <span className="badge dim">{s.status}</span>
                    <button className="reload raw-btn" onClick={() => void onToggleRaw(s.id)}>
                      {rawLoading === s.id ? '…' : raws[s.id] ? '▾ 收起原始' : '▸ 原始内容'}
                    </button>
                  </div>
                  <div className="v2-item-desc">{s.goal}</div>
                  <div className="v2-item-caps">
                    {s.capabilities.map((c) => (
                      <code key={c} className="cap">
                        {c}
                      </code>
                    ))}
                  </div>
                  {raws[s.id] && (
                    <div className="raw-box">
                      <div className="raw-meta">
                        <code>{raws[s.id].source_type}</code>
                        <code>hash {raws[s.id].content_hash.slice(0, 12)}…</code>
                      </div>
                      <pre className="raw-content">{raws[s.id].content}</pre>
                    </div>
                  )}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}

      {tab === 'plugins' && (
        <div className="v2-section">
          <div className="v2-head">
            <h3>Plugin Registry —「谁来提供能力」</h3>
            <button className="reload" onClick={() => void reloadPlugins()} disabled={loading}>
              {loading ? '刷新中…' : '↻ 刷新'}
            </button>
          </div>
          {plugins.length === 0 ? (
            <div className="empty">
              暂无 Plugin。插件由 Rust 侧 install/install_async 注册（真实网络：
              HttpPlugin / McpPlugin / AI Provider）。
            </div>
          ) : (
            <ul className="v2-list">
              {plugins.map((p) => (
                <li key={p.id} className="v2-item">
                  <div className="v2-item-head">
                    <code>{p.id}</code>
                    <span className="badge">{p.runtime}</span>
                    <span className="badge dim">{p.status}</span>
                  </div>
                  {p.endpoint && <div className="v2-item-desc">endpoint: {p.endpoint}</div>}
                  <div className="v2-item-caps">
                    {p.capabilities.map((c) => (
                      <code key={c} className="cap">
                        {c}
                      </code>
                    ))}
                  </div>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}

      {tab === 'sessions' && (
        <div className="v2-section">
          <div className="v2-head">
            <h3>Session Event Log —「到底发生了什么」（append-only）</h3>
            <div>
              <button className="reload" onClick={onNewSession} disabled={loading}>
                ＋ 新 Session
              </button>
              <button
                className="reload"
                onClick={() => session && void reloadSession(session.session_id)}
                disabled={loading || !session}
              >
                ↻ 刷新当前 Session
              </button>
            </div>
          </div>
          {!session ? (
            <div className="empty">暂无 Session。创建后事件流在此展示（可 Replay）。</div>
          ) : (
            <ol className="v2-session">
              {session.events.map((e) => (
                <li key={e.seq} className="session-event">
                  <span className="seq">#{e.seq}</span>
                  <code className="ev-type">{e.event_type}</code>
                  <pre className="ev-data">{JSON.stringify(e.data)}</pre>
                </li>
              ))}
            </ol>
          )}
        </div>
      )}
    </div>
  )
}
