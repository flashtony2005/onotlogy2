//! 项目管理面板（Phase 8 UI）：项目列表 + 创建 + seed + 项目看板。
//!
//! 项目 = 世界的顶层组织单元：每个项目有自己的世界（对象/关系/事件）。
//! - 列表：id / name / status / ontology / 世界规模（world.query total）
//! - 创建：id / name / description / ontology（ODL）
//! - seed：OpenAIP-OS 工程本体一键写入项目世界（模块/文件/测试）
//! - 看板：选中项目的世界关系（含 fabric.status 领域状态）+ 审计事件流

import { useEffect, useState } from 'react'
import {
  Activity,
  Database,
  FolderKanban,
  Plus,
  RefreshCw,
  Search,
  Sprout,
} from 'lucide-react'
import { postAuditQuery, postProjectCreate, postProjectList, postProjectSeed, postWorldQuery } from '../api'
import { fabricEventLabel } from '../format'
import type { AuditViewDto, ProjectDto, WorldViewDto } from '../types'

export function ProjectPanel() {
  const [projects, setProjects] = useState<ProjectDto[]>([])
  const [loading, setLoading] = useState(true)
  const [err, setErr] = useState<string | null>(null)
  const [selected, setSelected] = useState<ProjectDto | null>(null)
  const [seeding, setSeeding] = useState(false)

  // 创建表单
  const [showCreate, setShowCreate] = useState(false)
  const [newId, setNewId] = useState('')
  const [newName, setNewName] = useState('')
  const [newDesc, setNewDesc] = useState('')

  // 看板数据
  const [world, setWorld] = useState<WorldViewDto | null>(null)
  const [audit, setAudit] = useState<AuditViewDto | null>(null)

  const load = async () => {
    setLoading(true)
    setErr(null)
    try {
      const list = await postProjectList()
      setProjects(list)
      // 保持选中
      setSelected((prev) => {
        if (!prev) return null
        return list.find((p) => p.id === prev.id) ?? null
      })
    } catch (e) {
      setErr(String(e))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    void load()
  }, [])

  // 选中项目 → 拉项目世界 + 项目审计
  useEffect(() => {
    if (!selected) {
      setWorld(null)
      setAudit(null)
      return
    }
    let cancelled = false
    ;(async () => {
      try {
        const [w, a] = await Promise.all([
          postWorldQuery({ projectId: selected.id, limit: 300 }),
          postAuditQuery({ projectId: selected.id, limit: 100 }),
        ])
        if (!cancelled) {
          setWorld(w)
          setAudit(a)
        }
      } catch (e) {
        if (!cancelled) setErr(String(e))
      }
    })()
    return () => {
      cancelled = true
    }
  }, [selected])

  const create = async () => {
    setErr(null)
    try {
      if (!newId.trim() || !newName.trim()) throw new Error('id 与 name 必填')
      await postProjectCreate({
        id: newId.trim(),
        name: newName.trim(),
        description: newDesc.trim() || undefined,
      })
      setShowCreate(false)
      setNewId('')
      setNewName('')
      setNewDesc('')
      await load()
    } catch (e) {
      setErr(String(e))
    }
  }

  const seed = async () => {
    setSeeding(true)
    setErr(null)
    try {
      const r = await postProjectSeed()
      setErr(`✅ seed 完成：${r.events} 条世界事件 → OpenAIP-OS 工程`)
      await load()
      const fresh = await postProjectList()
      const oa = fresh.find((p) => p.id === 'openaip-os')
      if (oa) setSelected(oa)
    } catch (e) {
      setErr(String(e))
    } finally {
      setSeeding(false)
    }
  }

  // 项目世界统计（对象/关系类型分布）
  const typeCounts = new Map<string, number>()
  if (world) {
    for (const r of world.relations) {
      typeCounts.set(r.predicate, (typeCounts.get(r.predicate) ?? 0) + 1)
    }
  }
  const fabricRels = world?.relations.filter((r) => r.predicate.startsWith('fabric.')) ?? []

  return (
    <div className="world">
      <div className="dash-head">
        <div>
          <h3>项目管理</h3>
          <span className="dash-meta">
            Project = 世界顶层组织单元 · 项目 {projects.length} 个
          </span>
        </div>
        <div className="world-mode-tabs">
          <button onClick={() => void seed()} disabled={seeding}>
            <Sprout size={13} /> {seeding ? 'seed 中…' : 'seed OpenAIP-OS 工程'}
          </button>
          <button onClick={() => setShowCreate((v) => !v)}>
            <Plus size={13} /> 创建项目
          </button>
          <button className="reload" onClick={() => void load()} disabled={loading}>
            <RefreshCw size={13} /> 刷新
          </button>
        </div>
      </div>

      {err && <div className="err-msg">⚠️ {err}</div>}

      {showCreate && (
        <section className="dash-section">
          <h4>
            <Plus size={14} /> 创建项目（项目世界注册）
          </h4>
          <div className="world-filter">
            <input placeholder="id（如 my-project）" value={newId} onChange={(e) => setNewId(e.target.value)} />
            <input placeholder="name（如 我的项目）" value={newName} onChange={(e) => setNewName(e.target.value)} />
          </div>
          <div className="world-filter">
            <input placeholder="description（可选）" value={newDesc} onChange={(e) => setNewDesc(e.target.value)} />
            <button onClick={() => void create()}>创建</button>
          </div>
        </section>
      )}

      <div className="world-cols">
        {/* 左列：项目列表 */}
        <div className="fabric-col" style={{ maxWidth: 300 }}>
          <section className="dash-section">
            <h4>
              <FolderKanban size={14} /> 项目列表
            </h4>
            {projects.length === 0 ? (
              <div className="empty-sm">
                暂无项目。点击右上「seed OpenAIP-OS 工程」把本工程纳入本体论管理。
              </div>
            ) : (
              <div className="project-list">
                {projects.map((p) => (
                  <button
                    key={p.id}
                    className={`project-card ${selected?.id === p.id ? 'on' : ''}`}
                    onClick={() => setSelected(p)}
                  >
                    <span className="project-card-name">{p.name}</span>
                    <code className="project-card-id">{p.id}</code>
                    <span className="project-card-meta">
                      {p.status} · {p.ontology.length > 0 ? '有本体' : '无本体'}
                    </span>
                  </button>
                ))}
              </div>
            )}
          </section>
        </div>

        {/* 右列：项目看板 */}
        <div className="fabric-col">
          {!selected ? (
            <section className="dash-section">
              <div className="empty">
                选择左侧项目查看其世界与审计；或 seed OpenAIP-OS 工程开始本体论管理本工程。
              </div>
            </section>
          ) : (
            <>
              <section className="dash-section">
                <h4>
                  <Database size={14} /> {selected.name} · 项目世界
                </h4>
                <div className="dash-meta">
                  共 {world?.total ?? 0} 条关系
                  {fabricRels.length > 0 && ` · fabric 领域状态 ${fabricRels.length} 条（提交即世界）`}
                </div>
                {typeCounts.size > 0 && (
                  <div className="obj-grid">
                    {[...typeCounts.entries()]
                      .sort((a, b) => b[1] - a[1])
                      .slice(0, 8)
                      .map(([pred, count]) => (
                        <div key={pred} className="obj-card">
                          <span
                            className="obj-ic"
                            style={{ color: pred.startsWith('fabric.') ? '#e8a04c' : '#3fae7f' }}
                          >
                            <Search size={15} strokeWidth={2} />
                          </span>
                          <span className="obj-count">{count}</span>
                          <span className="obj-label">{pred}</span>
                        </div>
                      ))}
                  </div>
                )}
                {world && world.relations.length > 0 && (
                  <table className="world-rel-table">
                    <thead>
                      <tr>
                        <th>source</th>
                        <th>predicate</th>
                        <th>target</th>
                      </tr>
                    </thead>
                    <tbody>
                      {world.relations.slice(0, 12).map((r, i) => (
                        <tr key={i}>
                          <td>
                            <code>{r.source}</code>
                          </td>
                          <td className={r.predicate.startsWith('fabric.') ? 'world-pred-fabric' : ''}>
                            {r.predicate}
                          </td>
                          <td>
                            <code>{r.target}</code>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                )}
              </section>

              <section className="dash-section">
                <h4>
                  <Activity size={14} /> 项目审计（fabric.* / approval.* / policy.denied）
                </h4>
                {!audit || audit.events.length === 0 ? (
                  <div className="empty-sm">该项目暂无审计事件。</div>
                ) : (
                  <div className="fabric-events">
                    {audit.events.slice(0, 12).map((e) => (
                      <div key={e.id} className="fabric-ev">
                        <div className="fabric-ev-head">
                          <span className="fabric-ev-ty">{fabricEventLabel(e.eventType)}</span>
                          <code className="fabric-ev-refs">{e.refs.join(' → ')}</code>
                        </div>
                        <div className="fabric-ev-meta">{e.eventType} · {e.timestamp}</div>
                      </div>
                    ))}
                  </div>
                )}
              </section>
            </>
          )}
        </div>
      </div>
    </div>
  )
}
