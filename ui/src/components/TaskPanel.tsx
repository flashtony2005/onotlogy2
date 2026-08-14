import type { TaskRecord } from '../types'

export function TaskPanel({
  tasks,
  loading,
  onReload,
}: {
  tasks: TaskRecord[]
  loading: boolean
  onReload: () => void
}) {
  return (
    <div className="memory">
      <div className="memory-head">
        <h3>📌 任务状态（本体 RetentionTask）</h3>
        <button className="reload" onClick={onReload} disabled={loading}>
          {loading ? '刷新中…' : '↻ 刷新'}
        </button>
      </div>
      {tasks.length === 0 ? (
        <div className="empty">
          暂无任务。触发「manage retention tasks」类目标后，Planner 会创建
          <code>RetentionTask</code> 并推进 Pending → InProgress → Completed。
        </div>
      ) : (
        <div className="memory-list">
          {tasks.map((t) => (
            <article key={t.id} className="memory-item">
              <div className="mem-head">
                <code className="mem-label">{t.id}</code>
                <span className={`task-status ${t.status.toLowerCase()}`}>{t.status || '—'}</span>
                {t.priority && <span className="mem-hits">{t.priority}</span>}
              </div>
              <p className="task-title">{t.title || '（无标题）'}</p>
              <p className="task-meta">客户 {t.customer || '—'}</p>
            </article>
          ))}
        </div>
      )}
    </div>
  )
}
