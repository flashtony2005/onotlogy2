import { useEffect, useState } from 'react'
import { RefreshCw, ShieldCheck } from 'lucide-react'
import { AuthError, getApprovalAudit } from '../api'
import type { ApprovalAuditDto } from '../types'

export function GovernancePanel() {
  const [audits, setAudits] = useState<ApprovalAuditDto[] | null>(null)
  const [loading, setLoading] = useState(false)
  const [err, setErr] = useState<string | null>(null)

  const reload = async () => {
    setLoading(true)
    setErr(null)
    try {
      setAudits(await getApprovalAudit())
    } catch (e) {
      if (!(e instanceof AuthError)) setErr((e as Error).message || '审计加载失败')
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    void reload()
  }, [])

  return (
    <div className="evolution">
      <div className="evo-head">
        <h3>
          <ShieldCheck size={16} /> 治理 · 策略检查与审批审计
        </h3>
        <button className="reload" onClick={() => void reload()} disabled={loading}>
          <RefreshCw size={13} /> {loading ? '刷新中…' : '刷新'}
        </button>
      </div>

      <p className="evo-note">
        Learn≠Permission：知识不自动获得权限。策略允许、人工审批、自动放行资格均可追溯，
        审计记录可导出可校验。
      </p>

      {err && <div className="err-msg">⚠️ {err}</div>}

      <section className="evo-section">
        <h4>
          审批审计 <span className="evo-count">{audits?.length ?? 0}</span>
        </h4>
        {audits && audits.length === 0 && <div className="empty">暂无审批审计记录。</div>}
        {audits && audits.length > 0 && (
          <div>
            {audits.map((a) => (
              <article key={a.id} className="evo-item">
                <div className="evo-line">
                  <code className="evo-label">{a.action}</code>
                  <span className={`evo-tag decision ${a.decision}`}>{a.decision}</span>
                  <span className="evo-tag">run: {a.runId || '—'}</span>
                </div>
                <div className="evo-evidence">
                  {a.reason} · {a.createdAt}
                </div>
              </article>
            ))}
          </div>
        )}
      </section>
    </div>
  )
}
