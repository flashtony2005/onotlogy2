import { useState, useEffect, useMemo } from 'react'
import {
  CheckCircle,
  Search,
  Plus,
  Play,
  RotateCw,
} from 'lucide-react'
import {
  getGraph,
  postWorldQuery,
  getDomainPacks,
  setDomainPackEnabled,
  runDomainPackOperation,
  diagnoseKnowledge,
  getKnowledgeVersions,
  postProjectEvent,
} from '../api'
import type {
  WorldGraphDto,
  WorldRelationDto,
  DomainPackDto,
  KnowledgeDiagnosisDto,
  KnowledgeVersionDto,
} from '../types'
import { layoutGraph, colorForType } from '../graph'
import { fmtTime } from '../format'

export function WorldSpace() {
  const [graph, setGraph] = useState<WorldGraphDto>({ nodes: [], edges: [] })
  const [relations, setRelations] = useState<WorldRelationDto[]>([])
  const [packs, setPacks] = useState<DomainPackDto[]>([])
  const [diagnosis, setDiagnosis] = useState<KnowledgeDiagnosisDto | null>(null)
  const [versions, setVersions] = useState<KnowledgeVersionDto[]>([])
  const [selectedNode, setSelectedNode] = useState<{ id: string; type: string } | null>(null)
  const [filterType, setFilterType] = useState('All')
  const [search, setSearch] = useState('')
  const [addingEntity, setAddingEntity] = useState(false)
  const [newEntityId, setNewEntityId] = useState('')
  const [newEntityType, setNewEntityType] = useState('Customer')
  const [newEntityProps, setNewEntityProps] = useState('{"tier": "VIP_NEW", "churn_score": 0.5}')
  const [bannerMsg, setBannerMsg] = useState<string | null>(null)

  const loadData = async () => {
    try {
      const [g, w, p, diag, vers] = await Promise.all([
        getGraph(),
        postWorldQuery({}),
        getDomainPacks(),
        diagnoseKnowledge(),
        getKnowledgeVersions(),
      ])
      setGraph(g)
      setRelations(w.relations)
      setPacks(p)
      setDiagnosis(diag)
      setVersions(vers)
    } catch {
      /* ignore */
    }
  }

  useEffect(() => {
    loadData()
  }, [])

  const layout = useMemo(() => {
    return layoutGraph(graph.nodes)
  }, [graph])

  const types = useMemo(() => {
    const set = new Set(graph.nodes.map((n) => n.type))
    return ['All', ...Array.from(set)]
  }, [graph])

  const filteredNodes = useMemo(() => {
    return graph.nodes.filter((n) => {
      const matchType = filterType === 'All' || n.type === filterType
      const matchSearch =
        !search.trim() || n.id.toLowerCase().includes(search.toLowerCase())
      return matchType && matchSearch
    })
  }, [graph, filterType, search])

  const handleTogglePack = async (packId: string, current: boolean) => {
    await setDomainPackEnabled(packId, !current)
    await loadData()
    setBannerMsg(`已${!current ? '启用' : '停用'}业务规则包 [${packId}]`)
    setTimeout(() => setBannerMsg(null), 3000)
  }

  const handleRunPackOp = async (packId: string, op: any) => {
    await runDomainPackOperation(packId, op)
    setBannerMsg(`业务规则包 [${packId}] 操作 [${op}] 已触发执行`)
    setTimeout(() => setBannerMsg(null), 3000)
  }

  const handleCreateEntity = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!newEntityId.trim()) return
    let props = {}
    try {
      props = JSON.parse(newEntityProps)
    } catch {
      /* ignore */
    }
    await postProjectEvent({
      id: newEntityId.trim(),
      objectType: newEntityType,
      project: 'proj-crm-retention',
      input: props,
    })
    setAddingEntity(false)
    setNewEntityId('')
    await loadData()
    setBannerMsg(`已成功添加业务对象 [${newEntityId.trim()}]`)
    setTimeout(() => setBannerMsg(null), 3000)
  }

  return (
    <div className="space-root">
      {/* 空间头部 */}
      <div className="space-header">
        <div>
          <h2 className="space-title">
            <span className="space-icon">🌐</span> 业务图谱 · Business Knowledge Graph
          </h2>
          <p className="space-subtitle">
            全景可视化展示企业客户、预警信号、挽留策略与业务规则的真实网状关系
          </p>
        </div>

        <div style={{ display: 'flex', alignItems: 'center', gap: '12px' }}>
          <div
            style={{
              display: 'inline-flex',
              alignItems: 'center',
              gap: '6px',
              padding: '6px 12px',
              borderRadius: 'var(--radius-sm)',
              background: 'var(--success-light)',
              color: 'var(--success)',
              fontSize: '12px',
              fontWeight: 600,
            }}
            title={versions.length > 0 ? `最新修订版 Revision ${versions[0].revision}` : '规则健康'}
          >
            <CheckCircle size={14} />
            <span>{diagnosis?.consistent ? '业务规则校验正常' : '规则健康'}</span>
          </div>

          <button
            className="btn-action primary"
            onClick={() => setAddingEntity(true)}
          >
            <Plus size={14} /> 添加业务对象
          </button>
        </div>
      </div>

      {bannerMsg && (
        <div className="space-banner">
          <CheckCircle size={16} />
          <span>{bannerMsg}</span>
        </div>
      )}

      {/* 行业业务规则包快捷状态条 */}
      {packs.length > 0 && (
        <div style={{ display: 'flex', gap: '12px', marginBottom: '16px', flexWrap: 'wrap' }}>
          {packs.map((p) => (
            <div
              key={p.id}
              style={{
                flex: '1 1 320px',
                background: 'var(--surface)',
                border: '1px solid var(--border)',
                borderRadius: 'var(--radius-md)',
                padding: '10px 14px',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'space-between',
              }}
            >
              <div>
                <span style={{ fontWeight: 600, fontSize: '13px', color: 'var(--text-primary)' }}>
                  📦 {p.name}
                </span>
                <span style={{ fontSize: '11px', color: 'var(--text-muted)', marginLeft: '8px' }}>
                  ({p.ontology_types} 类对象 · {p.bootstrap_relations} 条规则)
                </span>
              </div>
              <div style={{ display: 'flex', gap: '8px', alignItems: 'center' }}>
                <button
                  className="btn-action secondary small"
                  onClick={() => handleRunPackOp(p.id, 'scan')}
                  title="扫描并同步最新状态"
                >
                  <RotateCw size={11} /> 扫描
                </button>
                <button
                  className="btn-action secondary small"
                  onClick={() => handleRunPackOp(p.id, 'backtest')}
                  title="模拟测试运行"
                >
                  <Play size={11} /> 测试
                </button>
                <button
                  className={`toggle-switch ${p.enabled ? 'on' : 'off'}`}
                  onClick={() => handleTogglePack(p.id, p.enabled)}
                >
                  {p.enabled ? '已启用' : '已停用'}
                </button>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* 核心主区域：可视化图谱 */}
      <div className="graph-container" style={{ marginBottom: '20px' }}>
        <div className="graph-toolbar">
          <div className="filter-group">
            <span className="lbl">对象类型筛选:</span>
            {types.map((t) => (
              <button
                key={t}
                className={`chip-btn ${filterType === t ? 'active' : ''}`}
                onClick={() => setFilterType(t)}
              >
                {t}
              </button>
            ))}
          </div>
          <span style={{ fontSize: '12px', color: 'var(--text-muted)' }}>
            点击任意节点可查看详细属性与关系
          </span>
        </div>

        <div className="graph-svg-wrapper">
          <svg className="graph-canvas" viewBox="0 0 950 520">
            <defs>
              <marker
                id="arrow"
                viewBox="0 0 10 10"
                refX="18"
                refY="5"
                markerWidth="6"
                markerHeight="6"
                orient="auto-start-reverse"
              >
                <path d="M 0 0 L 10 5 L 0 10 z" fill="#8da399" />
              </marker>
            </defs>

            {/* 边 */}
            {graph.edges.map((e) => {
              const s = layout.get(e.source)
              const t = layout.get(e.target)
              if (!s || !t) return null
              const midX = (s.x + t.x) / 2
              const midY = (s.y + t.y) / 2 - 8
              return (
                <g key={e.id}>
                  <line
                    x1={s.x}
                    y1={s.y}
                    x2={t.x}
                    y2={t.y}
                    stroke="#c8ded5"
                    strokeWidth="1.8"
                    strokeDasharray="4 2"
                    markerEnd="url(#arrow)"
                  />
                  <text
                    x={midX}
                    y={midY}
                    fill="#6d857b"
                    fontSize="10"
                    textAnchor="middle"
                    className="edge-label"
                  >
                    {e.type}
                  </text>
                </g>
              )
            })}

            {/* 节点 */}
            {filteredNodes.map((node) => {
              const pos = layout.get(node.id)
              if (!pos) return null
              const color = colorForType(node.type)
              const isSelected = selectedNode?.id === node.id
              return (
                <g
                  key={node.id}
                  transform={`translate(${pos.x}, ${pos.y})`}
                  className="graph-node-group"
                  onClick={() => setSelectedNode(node)}
                  style={{ cursor: 'pointer' }}
                >
                  <circle
                    r={isSelected ? 22 : 18}
                    fill={color}
                    opacity={isSelected ? 1 : 0.88}
                    stroke={isSelected ? '#1b2d25' : '#ffffff'}
                    strokeWidth={isSelected ? 3 : 2}
                    className="node-circle"
                  />
                  <text
                    y={4}
                    fill="#ffffff"
                    fontSize="11"
                    fontWeight="bold"
                    textAnchor="middle"
                    pointerEvents="none"
                  >
                    {node.type.slice(0, 1)}
                  </text>
                  <text
                    y={30}
                    fill="#263a32"
                    fontSize="11"
                    textAnchor="middle"
                    className="node-text"
                  >
                    {node.id.split(':').pop()}
                  </text>
                </g>
              )
            })}
          </svg>

          {/* 对象详情侧抽屉 */}
          {selectedNode && (
            <div className="node-drawer">
              <div className="drawer-head">
                <h4>业务对象详情</h4>
                <button className="close-btn" onClick={() => setSelectedNode(null)}>
                  ✕
                </button>
              </div>
              <div className="drawer-body">
                <div className="kv">
                  <span className="k">对象编号 (ID)</span>
                  <span className="v mono">{selectedNode.id}</span>
                </div>
                <div className="kv">
                  <span className="k">对象分类 (Type)</span>
                  <span className="v badge" style={{ background: colorForType(selectedNode.type), color: '#fff' }}>
                    {selectedNode.type}
                  </span>
                </div>

                <h5 className="sub-title">关联关系 (Relations)</h5>
                <ul className="rel-list">
                  {graph.edges
                    .filter((e) => e.source === selectedNode.id || e.target === selectedNode.id)
                    .map((e) => (
                      <li key={e.id}>
                        <code>{e.source}</code> ➔ <b>{e.type}</b> ➔ <code>{e.target}</code>
                      </li>
                    ))}
                </ul>
              </div>
            </div>
          )}
        </div>
      </div>

      {/* 下方区域：业务数据关系清单 */}
      <div className="glass-card">
        <div className="card-header-row">
          <h3 className="card-title">
            <Search size={16} /> 业务关系与数据清单 (Data Relations)
          </h3>
          <div className="search-bar" style={{ maxWidth: '300px', margin: 0 }}>
            <Search size={14} />
            <input
              placeholder="搜索对象编号或关联关系..."
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              style={{ fontSize: '12px' }}
            />
          </div>
        </div>

        <div className="entities-table-card" style={{ maxHeight: '260px', overflowY: 'auto' }}>
          <table className="pure-table">
            <thead>
              <tr>
                <th>起始对象 (Source)</th>
                <th>关联关系 (Relation)</th>
                <th>目标对象 (Target)</th>
                <th>有效时效 (Validity)</th>
              </tr>
            </thead>
            <tbody>
              {relations
                .filter(
                  (r) =>
                    !search.trim() ||
                    r.source.toLowerCase().includes(search.toLowerCase()) ||
                    r.predicate.toLowerCase().includes(search.toLowerCase()) ||
                    r.target.toLowerCase().includes(search.toLowerCase()),
                )
                .map((r, i) => (
                  <tr key={i}>
                    <td><code>{r.source}</code></td>
                    <td><span className="pred-pill">{r.predicate}</span></td>
                    <td><code>{r.target}</code></td>
                    <td>{r.validUntil ? `截止 ${fmtTime(new Date(r.validUntil * 1000).toISOString())}` : '永久有效'}</td>
                  </tr>
                ))}
            </tbody>
          </table>
        </div>
      </div>

      {/* 添加业务对象弹窗 */}
      {addingEntity && (
        <div className="modal-backdrop">
          <div className="modal-card">
            <h3>添加新业务对象</h3>
            <form onSubmit={handleCreateEntity}>
              <label className="field-lbl">对象编号 (ID):</label>
              <input
                className="input-text"
                placeholder="例如 crm:Customer_9901"
                value={newEntityId}
                onChange={(e) => setNewEntityId(e.target.value)}
                autoFocus
              />

              <label className="field-lbl">对象类型 (Type):</label>
              <select
                className="select-input"
                value={newEntityType}
                onChange={(e) => setNewEntityType(e.target.value)}
              >
                <option value="Customer">客户 (Customer)</option>
                <option value="ChurnRisk">流失预警 (ChurnRisk)</option>
                <option value="RetentionStrategy">挽留策略 (RetentionStrategy)</option>
                <option value="Policy">安全规则 (Policy)</option>
                <option value="Worker">执行节点 (Worker)</option>
              </select>

              <label className="field-lbl">对象属性 (JSON 格式):</label>
              <textarea
                className="textarea-json"
                rows={3}
                value={newEntityProps}
                onChange={(e) => setNewEntityProps(e.target.value)}
              />

              <div className="modal-actions">
                <button
                  type="button"
                  className="btn-action secondary"
                  onClick={() => setAddingEntity(false)}
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
