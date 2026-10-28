import { useState, useEffect } from 'react'
import {
  Network,
  Shield,
  Send,
  Plus,
  Terminal,
  Activity,
  CheckCircle2,
} from 'lucide-react'
import {
  getWorkers,
  getSkills,
  getPlugins,
  postFabricSubmit,
  importSkill,
  getUsers,
} from '../api'
import type {
  WorkerDto,
  SkillDto,
  PluginDto,
  UserDto,
} from '../types'

interface FabricSpaceProps {
  currentUser: { username: string; role: string }
  onSwitchUser: (username: string) => void
}

export function FabricSpace({ currentUser, onSwitchUser }: FabricSpaceProps) {
  const [subTab, setSubTab] = useState<'agents' | 'workers' | 'skills' | 'users'>('agents')
  const [workers, setWorkers] = useState<WorkerDto[]>([])
  const [skills, setSkills] = useState<SkillDto[]>([])
  const [plugins, setPlugins] = useState<PluginDto[]>([])
  const [users, setUsers] = useState<UserDto[]>([])
  const [taskIntent, setTaskIntent] = useState('CustomerRetentionEvaluation')
  const [taskPayload, setTaskPayload] = useState('{"customerId": "crm:Customer_9482", "channel": "VIP_Phone"}')
  const [submitResult, setSubmitResult] = useState<string | null>(null)
  const [showSkillModal, setShowSkillModal] = useState(false)
  const [newSkillName, setNewSkillName] = useState('')
  const [newSkillContent, setNewSkillContent] = useState('')
  const [notice, setNotice] = useState<string | null>(null)

  const loadData = async () => {
    try {
      const [w, s, p, u] = await Promise.all([
        getWorkers(),
        getSkills(),
        getPlugins(),
        getUsers(),
      ])
      setWorkers(w)
      setSkills(s)
      setPlugins(p)
      setUsers(u)
    } catch {
      /* ignore */
    }
  }

  useEffect(() => {
    loadData()
  }, [])

  const handleSubmitTask = async (e: React.FormEvent) => {
    e.preventDefault()
    let payload = {}
    try {
      payload = JSON.parse(taskPayload)
    } catch {
      /* ignore */
    }
    const res = await postFabricSubmit({
      sessionId: `sess-${Date.now().toString(36)}`,
      sender: currentUser.username,
      intent: taskIntent,
      payload,
    })
    setSubmitResult(`任务已受理! 单号: ${res.result.id} | 接收智能体: ${res.result.receiver}`)
    setTimeout(() => setSubmitResult(null), 4000)
  }

  const handleImportSkill = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!newSkillName.trim()) return
    await importSkill(newSkillName.trim(), newSkillContent, ['custom.reasoning', 'agent.dispatch'])
    setShowSkillModal(false)
    setNewSkillName('')
    setNewSkillContent('')
    await loadData()
    setNotice(`新技能 [${newSkillName}] 添加成功！`)
    setTimeout(() => setNotice(null), 3000)
  }

  return (
    <div className="space-root">
      {/* 空间头部 */}
      <div className="space-header">
        <div>
          <h2 className="space-title">
            <span className="space-icon">⚡</span> 智能体与能力 · Agents & Capabilities
          </h2>
          <p className="space-subtitle">
            管理协同工作的 AI 智能体团队、执行节点、业务技能库与外部系统插件
          </p>
        </div>

        <div className="space-subnav">
          <button
            className={`space-tab-btn ${subTab === 'agents' ? 'active' : ''}`}
            onClick={() => setSubTab('agents')}
          >
            智能体团队
          </button>
          <button
            className={`space-tab-btn ${subTab === 'workers' ? 'active' : ''}`}
            onClick={() => setSubTab('workers')}
          >
            执行节点 ({workers.length})
          </button>
          <button
            className={`space-tab-btn ${subTab === 'skills' ? 'active' : ''}`}
            onClick={() => setSubTab('skills')}
          >
            技能库 ({skills.length})
          </button>
          <button
            className={`space-tab-btn ${subTab === 'users' ? 'active' : ''}`}
            onClick={() => setSubTab('users')}
          >
            成员与权限
          </button>
        </div>
      </div>

      {notice && (
        <div className="space-banner">
          <CheckCircle2 size={16} />
          <span>{notice}</span>
        </div>
      )}

      {/* ── 视图 1：智能体团队 (Agents) ─────────────────────────────────── */}
      {subTab === 'agents' && (
        <div className="fabric-grid">
          <div className="glass-card">
            <h3 className="card-title">
              <Network size={17} /> 活跃智能体列表 (Active Agents)
            </h3>
            <div className="agents-topology">
              <div className="agent-node-card">
                <div className="node-head">
                  <span className="dot on" />
                  <h4>retention-agent</h4>
                  <span className="role-tag">决策分析 (Reasoner)</span>
                </div>
                <p>专司客户留存分析、流失风险评估与决策规划，直连 CRM 数据源与优惠券中台。</p>
                <div className="node-meta">
                  <span>通信协议: OAB-v2</span>
                  <span>运行状态: 正常 (14 次/分)</span>
                </div>
              </div>

              <div className="agent-node-card">
                <div className="node-head">
                  <span className="dot on" />
                  <h4>lottery-agent</h4>
                  <span className="role-tag">数据分析 (Analytics)</span>
                </div>
                <p>专司历史业务数据清洗、概率转移模型计算与趋势模拟回测。</p>
                <div className="node-meta">
                  <span>通信协议: OAB-v2</span>
                  <span>运行状态: 空闲待命</span>
                </div>
              </div>
            </div>

            <h3 className="card-title" style={{ marginTop: '24px' }}>
              <Terminal size={17} /> 🚀 手动下发测试任务 (Dispatch Task)
            </h3>
            <form onSubmit={handleSubmitTask} className="task-form">
              <label className="field-lbl">任务类型 (Task Type):</label>
              <input
                className="input-text"
                value={taskIntent}
                onChange={(e) => setTaskIntent(e.target.value)}
              />

              <label className="field-lbl">任务参数 (JSON 格式):</label>
              <textarea
                className="textarea-json"
                rows={2}
                value={taskPayload}
                onChange={(e) => setTaskPayload(e.target.value)}
              />

              <button type="submit" className="btn-action primary">
                <Send size={14} /> 下发任务给智能体
              </button>
              {submitResult && <p className="success-tip">{submitResult}</p>}
            </form>
          </div>

          <div className="glass-card">
            <h3 className="card-title">
              <Activity size={17} /> 外部连接插件 (Plugins & Connectors)
            </h3>
            <div className="plugins-list">
              {plugins.map((pl) => (
                <div key={pl.id} className="plugin-item">
                  <div className="pl-head">
                    <b>{pl.name}</b>
                    <span className="runtime-tag">{pl.runtime}</span>
                  </div>
                  <span className="pl-ep">{pl.endpoint || '内置连接器 (Internal Connector)'}</span>
                  <div className="pl-caps">
                    {pl.capabilities.map((c) => (
                      <span key={c} className="cap-pill">{c}</span>
                    ))}
                  </div>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}

      {/* ── 视图 2：执行节点 (Workers) ─────────────────────────────────────── */}
      {subTab === 'workers' && (
        <div className="workers-grid">
          {workers.map((w) => (
            <div key={w.id} className="worker-card">
              <div className="worker-head">
                <div>
                  <h4 className="worker-type">{w.worker_type}</h4>
                  <span className="worker-id">{w.id}</span>
                </div>
                <span className={`worker-status ${w.status.toLowerCase()}`}>
                  {w.status}
                </span>
              </div>

              <div className="worker-quality-row">
                <div className="q-box">
                  <span className="lbl">任务成功率</span>
                  <span className="val hl">{(w.quality * 100).toFixed(1)}%</span>
                </div>
                <div className="q-box">
                  <span className="lbl">单次执行成本</span>
                  <span className="val">¥{w.cost.toFixed(2)} / 次</span>
                </div>
                <div className="q-box">
                  <span className="lbl">执行引擎</span>
                  <span className="val">{w.execution}</span>
                </div>
              </div>

              <div className="worker-caps">
                <span className="caps-title">具备能力清单:</span>
                <div className="caps-tags">
                  {w.capabilities.map((c) => (
                    <span key={c} className="cap-pill">{c}</span>
                  ))}
                </div>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* ── 视图 3：技能库 (Skills) ───────────────────────────────────────── */}
      {subTab === 'skills' && (
        <div className="skills-view">
          <div className="skills-toolbar">
            <p className="desc">
              智能体掌握的专业业务技能库，支持按业务场景自定义添加与动态更新。
            </p>
            <button
              className="btn-action primary small"
              onClick={() => setShowSkillModal(true)}
            >
              <Plus size={14} /> 添加新业务技能
            </button>
          </div>

          <div className="skills-grid">
            {skills.map((s) => (
              <div key={s.id} className="skill-card">
                <div className="skill-head">
                  <h4>{s.name}</h4>
                  <span className="ver-badge">v{s.version}</span>
                </div>
                <p className="skill-goal">{s.goal}</p>

                <div className="skill-caps">
                  {s.capabilities.map((c) => (
                    <span key={c} className="cap-pill green">{c}</span>
                  ))}
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* ── 视图 4：成员与权限 (Users) ─────────────────────────────────────── */}
      {subTab === 'users' && (
        <div className="users-view">
          <div className="glass-card">
            <h3 className="card-title">
              <Shield size={17} /> 快速切换当前工作身份
            </h3>
            <p className="card-desc">
              不同角色拥有不同的操作权限：开发者可管理技能与插件，运营者负责业务执行与审批，决策官查看全局大盘。
            </p>

            <div className="user-switch-pills">
              {users.map((u) => {
                const isCurrent = u.username === currentUser.username
                return (
                  <div
                    key={u.id}
                    className={`user-card ${isCurrent ? 'active' : ''}`}
                    onClick={() => onSwitchUser(u.username)}
                  >
                    <div className="u-avatar">{u.displayName.slice(0, 1)}</div>
                    <div className="u-meta">
                      <b>{u.displayName}</b>
                      <span>@{u.username}</span>
                      <span className={`role-pill role-${u.role.toLowerCase()}`}>{u.role}</span>
                    </div>
                    {isCurrent && <span className="active-tag">当前会话</span>}
                  </div>
                )
              })}
            </div>
          </div>
        </div>
      )}

      {/* 添加新技能弹窗 */}
      {showSkillModal && (
        <div className="modal-backdrop">
          <div className="modal-card">
            <h3>添加新业务技能</h3>
            <form onSubmit={handleImportSkill}>
              <label className="field-lbl">技能名称 (Name):</label>
              <input
                className="input-text"
                placeholder="例如 CustomerRetentionOptimizer"
                value={newSkillName}
                onChange={(e) => setNewSkillName(e.target.value)}
                autoFocus
              />

              <label className="field-lbl">技能逻辑与触发说明 (Content):</label>
              <textarea
                className="textarea-json"
                rows={4}
                placeholder="描述技能的触发条件、业务目标与执行逻辑..."
                value={newSkillContent}
                onChange={(e) => setNewSkillContent(e.target.value)}
              />

              <div className="modal-actions">
                <button
                  type="button"
                  className="btn-action secondary"
                  onClick={() => setShowSkillModal(false)}
                >
                  取消
                </button>
                <button type="submit" className="btn-action primary">
                  确认添加
                </button>
              </div>
            </form>
          </div>
        </div>
      )}
    </div>
  )
}
