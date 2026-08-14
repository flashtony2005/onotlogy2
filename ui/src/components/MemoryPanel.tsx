import type { MemoryRecord } from '../types'
import { fmtJson, fmtTime } from '../format'

export function MemoryPanel({
  memories,
  loading,
  onReload,
}: {
  memories: MemoryRecord[]
  loading: boolean
  onReload: () => void
}) {
  return (
    <div className="memory">
      <div className="memory-head">
        <h3>💾 记忆沉淀（Action → Memory → 下次 Plan 闭环）</h3>
        <button className="reload" onClick={onReload} disabled={loading}>
          {loading ? '刷新中…' : '↻ 刷新'}
        </button>
      </div>
      {memories.length === 0 ? (
        <div className="empty">
          暂无记忆。运行含动作执行的 Agent（如挽留计划）后，动作会按
          <code>action.&lt;name&gt;</code> 沉淀为记忆，hits 随复用轮次累加。
        </div>
      ) : (
        <div className="memory-list">
          {memories.map((m) => (
            <article key={m.label} className="memory-item">
              <div className="mem-head">
                <code className="mem-label">{m.label}</code>
                <span className="mem-hits" title="复用次数（自学习指标）">
                  复用 {m.hits}
                </span>
                <span className="mem-time">{fmtTime(m.updatedAt)}</span>
              </div>
              <pre className="mem-payload">{fmtJson(m.payload)}</pre>
            </article>
          ))}
        </div>
      )}
    </div>
  )
}
