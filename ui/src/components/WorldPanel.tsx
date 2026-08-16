import { useEffect, useMemo, useState } from 'react'
import { Boxes, Globe2, Network, RefreshCw } from 'lucide-react'
import { getGraph } from '../api'
import { fmtJson } from '../format'
import type { WorldGraphDto, WorldNodeDto } from '../types'

const TYPE_COLORS = [
  '#4c9ee6',
  '#3fae7f',
  '#e8a04c',
  '#9f8fd6',
  '#39d0d8',
  '#e06a63',
]

function colorForType(type: string): string {
  let h = 0
  for (const ch of type) h = (h * 31 + ch.charCodeAt(0)) >>> 0
  return TYPE_COLORS[h % TYPE_COLORS.length]
}

/** 简单确定性布局：每种类型一列，节点纵向均分；返回 id → 坐标。 */
function layoutGraph(nodes: WorldNodeDto[]): Map<string, { x: number; y: number }> {
  const types = [...new Set(nodes.map((n) => n.type))].sort()
  const byType = new Map<string, WorldNodeDto[]>()
  for (const t of types) byType.set(t, nodes.filter((n) => n.type === t))
  const W = 900
  const H = 520
  const colGap = types.length > 1 ? W / types.length : W
  const pos = new Map<string, { x: number; y: number }>()
  types.forEach((t, i) => {
    const list = byType.get(t) ?? []
    const step = Math.min(64, (H - 80) / Math.max(list.length, 1))
    list.forEach((n, j) => {
      const y = list.length === 1 ? H / 2 : 50 + step * j + step / 2
      pos.set(n.id, { x: colGap * (i + 0.5), y })
    })
  })
  return pos
}

export function WorldPanel() {
  const [graph, setGraph] = useState<WorldGraphDto | null>(null)
  const [loading, setLoading] = useState(true)
  const [err, setErr] = useState<string | null>(null)
  const [selectedId, setSelectedId] = useState<string | null>(null)

  const reload = async () => {
    setLoading(true)
    setErr(null)
    try {
      const g = await getGraph()
      setGraph(g)
      setSelectedId((prev) => (prev && g.nodes.some((n) => n.id === prev) ? prev : null))
    } catch (e) {
      setErr(String(e))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    void reload()
  }, [])

  const nodes = graph?.nodes ?? []
  const edges = graph?.edges ?? []
  const positions = useMemo(() => layoutGraph(nodes), [nodes])
  const typeCounts = useMemo(() => {
    const m = new Map<string, number>()
    for (const n of nodes) m.set(n.type, (m.get(n.type) ?? 0) + 1)
    return [...m.entries()].sort((a, b) => b[1] - a[1])
  }, [nodes])
  const selected = selectedId ? nodes.find((n) => n.id === selectedId) ?? null : null
  const selectedEdges = useMemo(() => {
    if (!selectedId) return []
    return edges
      .filter((e) => e.source === selectedId || e.target === selectedId)
      .map((e) => ({
        edge: e,
        direction: e.source === selectedId ? 'out' : 'in',
        other: e.source === selectedId ? e.target : e.source,
      }))
  }, [edges, selectedId])

  return (
    <div className="world">
      <div className="dash-head">
        <div>
          <h3>世界</h3>
          <span className="dash-meta">
            World-aware Agent Operating System · 对象 {nodes.length} · 关系 {edges.length}
          </span>
        </div>
        <button className="reload" onClick={() => void reload()} disabled={loading}>
          <RefreshCw size={13} /> {loading ? '加载中…' : '刷新'}
        </button>
      </div>

      {err && <div className="err-msg">⚠️ {err}</div>}
      {loading && <div className="empty-sm">❯ 世界图谱加载中…</div>}

      {!loading && nodes.length === 0 && (
        <div className="empty">
          世界为空。运行含动作的 Agent 或写入对象后，这里会出现对象类型、关系图与对象事件链。
        </div>
      )}

      {nodes.length > 0 && (
        <>
          <section className="dash-section">
            <h4>
              <Boxes size={14} /> 对象类型
            </h4>
            <div className="obj-grid">
              {typeCounts.map(([type, count]) => {
                const color = colorForType(type)
                return (
                  <button
                    key={type}
                    className="obj-card"
                    onClick={() => {
                      const first = nodes.find((n) => n.type === type)
                      if (first) setSelectedId(first.id)
                    }}
                  >
                    <span
                      className="obj-ic"
                      style={{ color, background: `color-mix(in srgb, ${color} 12%, white)` }}
                    >
                      <Globe2 size={18} strokeWidth={2} />
                    </span>
                    <span className="obj-count">{count}</span>
                    <span className="obj-label">{type}</span>
                  </button>
                )
              })}
            </div>
          </section>

          <section className="dash-section">
            <h4>
              <Network size={14} /> 对象关系图
            </h4>
            <div className="world-cols">
              <div className="world-graph">
                <svg viewBox="0 0 900 520" className="world-graph-svg" role="img" aria-label="世界对象关系图">
                  <defs>
                    <marker
                      id="edge-arrow"
                      viewBox="0 0 8 8"
                      refX="7"
                      refY="4"
                      markerWidth="7"
                      markerHeight="7"
                      orient="auto-start-reverse"
                    >
                      <path d="M0 0 L8 4 L0 8 Z" fill="#a9c3bb" />
                    </marker>
                  </defs>
                  {edges.map((e) => {
                    const s = positions.get(e.source)
                    const t = positions.get(e.target)
                    if (!s || !t) return null
                    return (
                      <g key={e.id}>
                        <line
                          x1={s.x}
                          y1={s.y}
                          x2={t.x}
                          y2={t.y}
                          className="world-edge"
                          markerEnd="url(#edge-arrow)"
                        />
                        <text
                          x={(s.x + t.x) / 2}
                          y={(s.y + t.y) / 2 - 7}
                          className="world-edge-label"
                          textAnchor="middle"
                        >
                          {e.type}
                        </text>
                      </g>
                    )
                  })}
                  {nodes.map((n) => {
                    const p = positions.get(n.id)
                    if (!p) return null
                    const color = colorForType(n.type)
                    const active = n.id === selectedId
                    return (
                      <g
                        key={n.id}
                        transform={`translate(${p.x} ${p.y})`}
                        className={`world-node ${active ? 'active' : ''}`}
                        onClick={() => setSelectedId(n.id)}
                      >
                        <circle r={17} fill={active ? color : '#fff'} stroke={color} strokeWidth={2} />
                        <text
                          y={4}
                          textAnchor="middle"
                          className={`world-node-ic ${active ? 'on' : ''}`}
                        >
                          {n.type.slice(0, 1).toUpperCase()}
                        </text>
                        <text y={34} textAnchor="middle" className="world-node-label">
                          {n.id}
                        </text>
                      </g>
                    )
                  })}
                </svg>
              </div>

              <aside className="world-detail">
                {selected ? (
                  <>
                    <div className="world-detail-head">
                      <span className="world-detail-type" style={{ color: colorForType(selected.type) }}>
                        {selected.type}
                      </span>
                      <code>{selected.id}</code>
                    </div>
                    <h4>为什么是这个状态</h4>
                    <div className="world-rel-list">
                      {selectedEdges.length === 0 && (
                        <div className="empty-sm">该对象暂无关系链。</div>
                      )}
                      {selectedEdges.map(({ edge, direction, other }) => (
                        <button
                          key={edge.id}
                          className="world-rel"
                          onClick={() => setSelectedId(other)}
                        >
                          <span className={`world-rel-arrow ${direction}`}>
                            {direction === 'out' ? '→' : '←'}
                          </span>
                          <span className="world-rel-type">{edge.type}</span>
                          <code>{other}</code>
                        </button>
                      ))}
                    </div>
                    {selected.data != null && (
                      <>
                        <h4>当前状态</h4>
                        <pre className="world-data">{fmtJson(selected.data)}</pre>
                      </>
                    )}
                  </>
                ) : (
                  <div className="empty-sm">
                    点击对象查看当前状态、关系链与为什么处于该状态。
                  </div>
                )}
              </aside>
            </div>
          </section>
        </>
      )}
    </div>
  )
}
