import { useState, useEffect } from 'react'
import {
  Network,
  Shield,
  Send,
  Plus,
  Terminal,
  Activity,
  CheckCircle2,
  Cpu,
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
  const [workers, setWorkers] = useState<WorkerDto[]>([])
  const [skills, setSkills] = useState<SkillDto[]>([])
  const [plugins, setPlugins] = useState<PluginDto[]>([])
  const [users, setUsers] = useState<UserDto[]>([])
  const [taskIntent, setTaskIntent] = useState('CrossBorderInventoryRebalance')
  const [taskPayload, setTaskPayload] = useState('{"skuId": "sku:AuraPro_GaN65W", "targetWarehouse": "wh:Warehouse_US_West", "emergencyAirFreightQty": 2500}')
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
      {/* 空间头部与身份切换 */}
      <div className="space-header">
        <div>
          <h2 className="space-title">
            <span className="space-icon">🤖</span> 智能团队与能力 · Agents & Tools
          </h2>
          <p className="space-subtitle">
            专职智能体分工、业务技能资产库、算力节点与外部系统连接器
          </p>
        </div>

        {/* 顶部紧凑的角色身份切换 */}
        <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
          <span style={{ fontSize: '12px', color: 'var(--text-muted)' }}>
            <Shield size={13} style={{ verticalAlign: 'middle', marginRight: '4px' }} />
            当前操作者:
          </span>
          <div style={{ display: 'flex', gap: '6px' }}>
            {users.map((u) => {
              const isCurrent = u.username === currentUser.username
              return (
                <button
                  key={u.id}
                  className={`chip-btn ${isCurrent ? 'active' : ''}`}
                  onClick={() => onSwitchUser(u.username)}
                  style={{ fontSize: '11px', padding: '4px 10px' }}
                >
                  {u.displayName} ({u.role})
                </button>
              )
            })}
          </div>
        </div>
      </div>

      {notice && (
        <div className="space-banner">
          <CheckCircle2 size={16} />
          <span>{notice}</span>
        </div>
      )}

      {/* 第一板块：专职智能体卡片 */}
      <div className="glass-card" style={{ marginBottom: '20px' }}>
        <div className="card-header-row">
          <h3 className="card-title">
            <Network size={17} /> 活跃智能体成员 (Active Agents Team)
          </h3>
          <span style={{ fontSize: '12px', color: 'var(--text-muted)' }}>专职分工 · 协同决策</span>
        </div>

        <div className="agents-topology">
          <div className="agent-node-card">
            <div className="node-head">
              <span className="dot on" />
              <h4>retention-agent</h4>
              <span className="role-tag">决策分析 (Reasoner)</span>
            </div>
            <p>专司客户留存分析、流失风险评估与决策规划，直连客户 CRM 数据源与优惠券中台。</p>
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
            <p>专司历史业务数据清洗、概率转移模型计算与规律回测模拟。</p>
            <div className="node-meta">
              <span>通信协议: OAB-v2</span>
              <span>运行状态: 空闲待命</span>
            </div>
          </div>
        </div>
      </div>

      {/* 第二板块：左右分栏 - 业务技能库 与 连接器/执行节点 */}
      <div className="cockpit-grid" style={{ marginBottom: '20px' }}>
        {/* 业务技能库 */}
        <div className="glass-card">
          <div className="card-header-row">
            <h3 className="card-title">
              <Cpu size={17} /> 业务技能库 (Skills Library)
            </h3>
            <button
              className="btn-action primary small"
              onClick={() => setShowSkillModal(true)}
            >
              <Plus size={13} /> 添加新技能
            </button>
          </div>

          <div className="skills-grid" style={{ maxHeight: '320px', overflowY: 'auto' }}>
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

        {/* 外部插件连接器 与 执行节点 */}
        <div className="glass-card">
          <div className="card-header-row">
            <h3 className="card-title">
              <Activity size={17} /> 外部连接插件与节点 (Integrations)
            </h3>
            <span style={{ fontSize: '12px', color: 'var(--text-muted)' }}>
              {plugins.length} 个插件 · {workers.length} 个节点
            </span>
          </div>

          <div style={{ display: 'flex', flexDirection: 'column', gap: '12px' }}>
            {/* 插件列表 */}
            <div className="plugins-list" style={{ maxHeight: '170px', overflowY: 'auto' }}>
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

            {/* 执行节点简要 */}
            <div style={{ borderTop: '1px solid var(--border)', paddingTop: '10px' }}>
              <span style={{ fontSize: '12px', fontWeight: 600, color: 'var(--text-secondary)' }}>
                执行引擎性能:
              </span>
              <div style={{ display: 'flex', gap: '8px', marginTop: '6px', flexWrap: 'wrap' }}>
                {workers.map((w) => (
                  <div
                    key={w.id}
                    style={{
                      flex: '1 1 140px',
                      background: 'var(--canvas-subtle)',
                      padding: '8px 10px',
                      borderRadius: 'var(--radius-sm)',
                      fontSize: '11px',
                    }}
                  >
                    <div style={{ fontWeight: 600, color: 'var(--text-primary)' }}>{w.worker_type}</div>
                    <div style={{ color: 'var(--text-muted)', marginTop: '2px' }}>
                      成功率: <b style={{ color: 'var(--accent)' }}>{(w.quality * 100).toFixed(0)}%</b> · ¥{w.cost.toFixed(2)}/次
                    </div>
                  </div>
                ))}
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* 第三板块：测试任务派发 */}
      <div className="glass-card">
        <div className="card-header-row">
          <h3 className="card-title">
            <Terminal size={17} /> 🚀 手动下发测试任务 (Dispatch Task)
          </h3>
          <span style={{ fontSize: '12px', color: 'var(--text-muted)' }}>
            用于开发者/运营人员快速测试智能体路由
          </span>
        </div>

        <form onSubmit={handleSubmitTask} className="task-form">
          <div style={{ display: 'flex', gap: '16px' }}>
            <div style={{ flex: '1' }}>
              <label className="field-lbl">任务类型 (Task Type):</label>
              <input
                className="input-text"
                value={taskIntent}
                onChange={(e) => setTaskIntent(e.target.value)}
              />
            </div>
            <div style={{ flex: '2' }}>
              <label className="field-lbl">任务参数 (JSON 格式):</label>
              <textarea
                className="textarea-json"
                rows={1}
                value={taskPayload}
                onChange={(e) => setTaskPayload(e.target.value)}
              />
            </div>
          </div>

          <div style={{ display: 'flex', alignItems: 'center', gap: '12px', marginTop: '10px' }}>
            <button type="submit" className="btn-action primary">
              <Send size={14} /> 下发任务给智能体
            </button>
            {submitResult && <p className="success-tip" style={{ margin: 0 }}>{submitResult}</p>}
          </div>
        </form>
      </div>

      {/* 添加新技能弹窗 */}
      {showSkillModal && (
        <div className="modal-backdrop">
          <div className="modal-card">
            <h3>添加新业务技能</h3>
            <form onSubmit={handleImportSkill}>
              <label className="field-lbl">技能名称 (Name):</label>
              <input
                className="input-text"
                placeholder="例如 CrossBorderStockoutOptimizer"
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
