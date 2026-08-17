// 图布局工具（WorldPanel / ProjectPanel 共用）：按类型分列确定性布局。

const TYPE_COLORS = [
  '#4c9ee6',
  '#3fae7f',
  '#e8a04c',
  '#9f8fd6',
  '#39d0d8',
  '#e06a63',
]

/** 确定性配色：同一类型恒定颜色（不引入随机，便于对比）。 */
export function colorForType(type: string): string {
  let h = 0
  for (const ch of type) h = (h * 31 + ch.charCodeAt(0)) >>> 0
  return TYPE_COLORS[h % TYPE_COLORS.length]
}

export interface GraphNode {
  id: string
  type: string
}

export type GraphPos = Map<string, { x: number; y: number }>

/** 简单确定性布局：每种类型一列，节点纵向均分；返回 id → 坐标。
 *  依赖图场景用 node.type = node.id，使每个节点独占一列 → 横向铺开。 */
export function layoutGraph(nodes: GraphNode[]): GraphPos {
  const types = [...new Set(nodes.map((n) => n.type))].sort()
  const byType = new Map<string, GraphNode[]>()
  for (const t of types) byType.set(t, nodes.filter((n) => n.type === t))
  const W = 900
  const H = 520
  const colGap = types.length > 1 ? W / types.length : W
  const pos: GraphPos = new Map()
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
