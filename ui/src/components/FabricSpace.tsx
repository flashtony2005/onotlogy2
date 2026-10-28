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
    setSubmitResult(`任务已受理! ID: ${res.result.id} | 接收 Agent: ${res.result.receiver}`)
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
    setNotice(`新技能 [${newSkillName}] 导入成功！`)
    setTimeout(() => setNotice(null), 3000)
  }

  return (
    <div className="space-root">
      {/* 空间头部 */}
      <div className="space-header">
        <div>
          <h2 className="space-title">
            <span className="space-icon">⚡</span> 智能资产与网格 · Fabric & Assets
          </h2>
          <p className="space-subtitle">
            分布式智能体通信拓扑、专业工人能力网格与技能资产库
          </p>
        </div>

        <div className="space-subnav">
          <button
            className={`space-tab-btn ${subTab === 'agents' ? 'active' : ''}`}
            onClick={() => setSubTab('agents')}
          >
            Agent 通信网格
          </button>
          <button
            className={`space-tab-btn ${subTab === 'workers' ? 'active' : ''}`}
            onClick={() => setSubTab('workers')}
          >
            工人网格 ({workers.length})
          </button>
          <button
            className={`space-tab-btn ${subTab === 'skills' ? 'active' : ''}`}
            onClick={() => setSubTab('skills')}
          >
            技能资产库 ({skills.length})
          </button>
          <button
            className={`space-tab-btn ${subTab === 'users' ? 'active' : ''}`}
            onClick={() => setSubTab('users')}
          >
            身份与权限角色
          </button>
        </div>
      </div>

      {notice && (
        <div className="space-banner">
          <CheckCircle2 size={16} />
          <span>{notice}</span>
        </div>
      )}

      {/* ── 视图 1：Agent 通信网格 (Fabric) ─────────────────────────────────── */}
      {subTab === 'agents' && (
        <div className="fabric-grid">
          <div className="glass-card">
            <h3 className="card-title">
              <Network size={17} /> 活跃智能体拓扑 (Active Agent Topology)
            </h3>
            <div className="agents-topology">
              <div className="agent-node-card">
                <div className="node-head">
                  <span className="dot on" />
                  <h4>retention-agent</h4>
                  <span className="role-tag">Cognitive Reasoner</span>
                </div>
                <p>专司客户留存分析、流失风险评估与决策规划，直连 CRM 数据源与优惠券中台。</p>
                <div className="node-meta">
                  <span>协议: OAB-v2</span>
                  <span>负载: 正常 (14 ops/m)</span>
                </div>
              </div>

              <div className="agent-node-card">
                <div className="node-head">
                  <span className="dot on" />
                  <h4>lottery-agent</h4>
                  <span className="role-tag">Analytics Engine</span>
                </div>
                <p>专司开奖大数据清洗、马尔可夫转移矩阵计算与策略回测。</p>
                <div className="node-meta">
                  <span>协议: OAB-v2</span>
                  <span>负载: 空闲</span>
                </div>
              </div>
            </div>

            <h3 className="card-title" style={{ marginTop: '24px' }}>
              <Terminal size={17} /> 经 Fabric 网关派发任务
            </h3>
            <form onSubmit={handleSubmitTask} className="task-form">
              <label className="field-lbl">意图识别码 (Task Intent):</label>
              <input
                className="input-text"
                value={taskIntent}
                onChange={(e) => setTaskIntent(e.target.value)}
              />

              <label className="field-lbl">载荷参数 (Payload):</label>
              <textarea
                className="textarea-json"
                rows={2}
                value={taskPayload}
                onChange={(e) => setTaskPayload(e.target.value)}
              />

              <button type="submit" className="btn-action primary">
                <Send size={14} /> 提交到 Agent Fabric
              </button>
              {submitResult && <p className="success-tip">{submitResult}</p>}
            </form>
          </div>

          <div className="glass-card">
            <h3 className="card-title">
              <Activity size={17} /> 外部连接插件 (Plugins)
            </h3>
            <div className="plugins-list">
              {plugins.map((pl) => (
                <div key={pl.id} className="plugin-item">
                  <div className="pl-head">
                    <b>{pl.name}</b>
                    <span className="runtime-tag">{pl.runtime}</span>
                  </div>
                  <span className="pl-ep">{pl.endpoint || 'Internal Connector'}</span>
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

      {/* ── 视图 2：工人网格 (Workers) ─────────────────────────────────────── */}
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
                  <span className="lbl">执行质量</span>
                  <span className="val hl">{(w.quality * 100).toFixed(1)}%</span>
                </div>
                <div className="q-box">
                  <span className="lbl">执行成本</span>
                  <span className="val">¥{w.cost.toFixed(2)} / op</span>
                </div>
                <div className="q-box">
                  <span className="lbl">执行引擎</span>
                  <span className="val">{w.execution}</span>
                </div>
              </div>

              <div className="worker-caps">
                <span className="caps-title">挂载能力清单:</span>
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

      {/* ── 视图 3：技能资产库 (Skills) ───────────────────────────────────── */}
      {subTab === 'skills' && (
        <div className="skills-view">
          <div className="skills-toolbar">
            <p className="desc">
              系统执行特定领域任务的纯函数认知契约，支持动态热导入与版本追溯。
            </p>
            <button
              className="btn-action primary small"
              onClick={() => setShowSkillModal(true)}
            >
              <Plus size={14} /> 导入新 Skill
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

      {/* ── 视图 4：用户身份与角色 (Users) ─────────────────────────────────── */}
      {subTab === 'users' && (
        <div className="users-view">
          <div className="glass-card">
            <h3 className="card-title">
              <Shield size={17} /> 快速切换当前工作身份
            </h3>
            <p className="card-desc">
              不同角色拥有不同的治理权限（如仅 Developer 可导入 Skill，Operator 负责业务放行）。
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

      {/* 导入 Skill 弹窗 */}
      {showSkillModal && (
        <div className="modal-backdrop">
          <div className="modal-card">
            <h3>导入领域认知 Skill</h3>
            <form onSubmit={handleImportSkill}>
              <label className="field-lbl">技能名称 (Name):</label>
              <input
                className="input-text"
                placeholder="例如 CustomerRetentionOptimizer"
                value={newSkillName}
                onChange={(e) => setNewSkillName(e.target.value)}
                autoFocus
              />

              <label className="field-lbl">技能规范说明或代码 (Content):</label>
              <textarea
                className="textarea-json"
                rows={4}
                placeholder="定义目标、触发条件与执行指令..."
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
                  确认导入
                </button>
              </div>
            </form>
          </div>
        </div>
      )}
    </div>
  )
}
