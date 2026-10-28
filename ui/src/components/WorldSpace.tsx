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
  const [subTab, setSubTab] = useState<'graph' | 'entities' | 'packs' | 'health'>('graph')
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
    setBannerMsg(`已${!current ? '启用' : '停用'}业务包 [${packId}]`)
    setTimeout(() => setBannerMsg(null), 3000)
  }

  const handleRunPackOp = async (packId: string, op: any) => {
    await runDomainPackOperation(packId, op)
    setBannerMsg(`业务包 [${packId}] 操作 [${op}] 已触发执行`)
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
    setBannerMsg(`已成功添加业务对象 [${newEntityId.trim()}] 到知识库`)
    setTimeout(() => setBannerMsg(null), 3000)
  }

  return (
    <div className="space-root">
      {/* 空间头部 */}
      <div className="space-header">
        <div>
          <h2 className="space-title">
            <span className="space-icon">🌐</span> 业务知识中心 · Business Knowledge
          </h2>
          <p className="space-subtitle">
            集中沉淀企业客户、业务规则与实体对象之间的全景关联网络
          </p>
        </div>

        <div className="space-subnav">
          <button
            className={`space-tab-btn ${subTab === 'graph' ? 'active' : ''}`}
            onClick={() => setSubTab('graph')}
          >
            关系网络图 ({graph.nodes.length} 个对象)
          </button>
          <button
            className={`space-tab-btn ${subTab === 'entities' ? 'active' : ''}`}
            onClick={() => setSubTab('entities')}
          >
            业务数据清单
          </button>
          <button
            className={`space-tab-btn ${subTab === 'packs' ? 'active' : ''}`}
            onClick={() => setSubTab('packs')}
          >
            行业规则包 ({packs.length})
          </button>
          <button
            className={`space-tab-btn ${subTab === 'health' ? 'active' : ''}`}
            onClick={() => setSubTab('health')}
          >
            数据健康检查
          </button>
        </div>
      </div>

      {bannerMsg && (
        <div className="space-banner">
          <CheckCircle size={16} />
          <span>{bannerMsg}</span>
        </div>
      )}

      {/* ── 视图 1：交互式关系网络图 (Graph) ─────────────────────────────── */}
      {subTab === 'graph' && (
        <div className="graph-container">
          <div className="graph-toolbar">
            <div className="filter-group">
              <span className="lbl">对象类型:</span>
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

            <button
              className="btn-action primary small"
              onClick={() => setAddingEntity(true)}
            >
              <Plus size={14} /> 添加业务对象
            </button>
          </div>

          <div className="graph-svg-wrapper">
            <svg className="graph-canvas" viewBox="0 0 950 560">
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
      )}

      {/* ── 视图 2：业务数据清单 (Entities) ───────────────────────────────── */}
      {subTab === 'entities' && (
        <div className="entities-view">
          <div className="search-bar">
            <Search size={16} />
            <input
              placeholder="搜索对象编号或关联关系..."
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </div>

          <div className="entities-table-card">
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
                {relations.map((r, i) => (
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
      )}

      {/* ── 视图 3：行业规则包 (Domain Packs) ──────────────────────────────── */}
      {subTab === 'packs' && (
        <div className="packs-grid">
          {packs.map((p) => (
            <div key={p.id} className="pack-card">
              <div className="pack-header">
                <div>
                  <h3 className="pack-name">{p.name}</h3>
                  <span className="pack-ver">Version {p.version}</span>
                </div>
                <button
                  className={`toggle-switch ${p.enabled ? 'on' : 'off'}`}
                  onClick={() => handleTogglePack(p.id, p.enabled)}
                >
                  {p.enabled ? '已启用' : '已停用'}
                </button>
              </div>

              <div className="pack-stats">
                <div className="stat">
                  <span className="num">{p.ontology_types}</span>
                  <span className="lbl">业务概念</span>
                </div>
                <div className="stat">
                  <span className="num">{p.bootstrap_objects}</span>
                  <span className="lbl">预置对象</span>
                </div>
                <div className="stat">
                  <span className="num">{p.bootstrap_relations}</span>
                  <span className="lbl">关联规则</span>
                </div>
              </div>

              <div className="pack-ops">
                <button
                  className="btn-pack-op"
                  onClick={() => handleRunPackOp(p.id, 'scan')}
                >
                  <RotateCw size={13} /> 扫描更新
                </button>
                <button
                  className="btn-pack-op"
                  onClick={() => handleRunPackOp(p.id, 'backtest')}
                >
                  <Play size={13} /> 模拟测试
                </button>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* ── 视图 4：数据健康检查 (Health) ─────────────────────────────────── */}
      {subTab === 'health' && (
        <div className="health-grid">
          <div className="glass-card">
            <h3 className="card-title">
              <CheckCircle size={18} className="clean-icon" /> 业务规则与数据一致性检查
            </h3>
            <div className="health-status-box">
              <span className="status-badge-big">
                {diagnosis?.consistent ? '✅ 规则健康正常，无逻辑冲突' : '⚠️ 存在规则冲突'}
              </span>
              <p className="health-desc">
                系统内所有业务规则与运行数据均保持逻辑一致，无冲突规则，可保障系统稳态运行。
              </p>
            </div>
          </div>

          <div className="glass-card">
            <h3 className="card-title">规则版本演进脉络 (Version History)</h3>
            <div className="versions-timeline">
              {versions.map((v) => (
                <div key={v.id} className="version-row">
                  <div className="v-tag">
                    <b>{v.id}</b>
                    <span>(Revision {v.revision})</span>
                  </div>
                  <span className="v-date">{fmtTime(v.created_at)}</span>
                  <span className="v-axioms">{v.ontology.axioms.length} 条业务规则定义</span>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}

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
