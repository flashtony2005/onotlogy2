import { useState } from 'react'
import { AuthError, getApprovalAudit, postAutoApproveToggle } from '../api'
import type { ApprovalAuditDto, EvolutionStateDto } from '../types'

export function EvolutionPanel({
  state,
  loading,
  onReload,
  onAuthError,
}: {
  state: EvolutionStateDto | null
  loading: boolean
  onReload: () => void
  /** 401（会话过期）→ 回登录页 */
  onAuthError?: () => void
}) {
  const [audits, setAudits] = useState<ApprovalAuditDto[] | null>(null)
  const [busyId, setBusyId] = useState<string | null>(null)
  const [panelError, setPanelError] = useState<string | null>(null)

  const total =
    (state?.patterns.length ?? 0) +
    (state?.proposals.length ?? 0) +
    (state?.learned.length ?? 0)

  // 切换自动放行资格（REQ-018 可撤销，幂等）；成功后刷新 Evolution + 审计。
  const toggleAutoApprove = async (id: string, enabled: boolean) => {
    setBusyId(id)
    setPanelError(null)
    try {
      await postAutoApproveToggle(id, enabled)
      await onReload()
      const a = await getApprovalAudit()
      setAudits(a)
    } catch (e) {
      if (e instanceof AuthError) onAuthError?.()
      else setPanelError((e as Error).message || '操作失败')
    } finally {
      setBusyId(null)
    }
  }

  const showAudit = async () => {
    setPanelError(null)
    try {
      const a = await getApprovalAudit()
      setAudits(a)
    } catch (e) {
      if (e instanceof AuthError) onAuthError?.()
      else setPanelError((e as Error).message || '审计加载失败')
    }
  }

  return (
    <div className="evolution">
      <div className="evo-head">
        <h3>🧬 Evolution · Experience→Knowledge 闭环</h3>
        <button className="reload" onClick={onReload} disabled={loading}>
          {loading ? '刷新中…' : '↻ 刷新'}
        </button>
      </div>
      <p className="evo-note">
        Learn≠Permission：本视图仅暴露知识，<b>不修改 Policy、不绕过 Human Approval（WB2005）</b>。
        证据链 Learned→Proposal→Pattern→Experience 全程可追溯。
      </p>

      {panelError && <div className="error">⚠ {panelError}</div>}

      {total === 0 ? (
        <div className="empty">
          暂无知识。运行多轮含复现动作的 Agent 后，经验会经 wb_memory 提炼为
          Pattern（hits≥2）→ Proposal → Validated Knowledge。
        </div>
      ) : (
        <div className="evo-body">
          <section className="evo-section">
            <h4>
              Pattern <span className="evo-count">{state?.patterns.length ?? 0}</span>
            </h4>
            {(state?.patterns ?? []).map((p) => (
              <article key={p.label} className="evo-item">
                <div className="evo-line">
                  <code className="evo-label">{p.label}</code>
                  <span className="evo-tag action">action: {p.action}</span>
                  <span className="evo-tag conf">conf {p.confidence.toFixed(2)}</span>
                </div>
                <div className="evo-evidence">
                  来源 run：{p.runIds.length ? p.runIds.join(', ') : '—'}
                </div>
              </article>
            ))}
          </section>

          <section className="evo-section">
            <h4>
              Proposal <span className="evo-count">{state?.proposals.length ?? 0}</span>
            </h4>
            {(state?.proposals ?? []).map((p) => (
              <article key={p.id} className="evo-item">
                <div className="evo-line">
                  <code className="evo-label">{p.id}</code>
                  <span className={`evo-tag status ${p.status.toLowerCase()}`}>{p.status}</span>
                  <span className="evo-tag conf">conf {p.confidence.toFixed(2)}</span>
                </div>
                <div className="evo-evidence">
                  来源 Pattern：{p.patternLabel} · run：
                  {p.runIds.length ? p.runIds.join(', ') : '—'}
                </div>
              </article>
            ))}
          </section>

          <section className="evo-section">
            <h4>
              Learned <span className="evo-count">{state?.learned.length ?? 0}</span>
            </h4>
            {(state?.learned ?? []).map((l) => (
              <article key={l.id} className="evo-item">
                <div className="evo-line">
                  <code className="evo-label">{l.id}</code>
                  <span className="evo-tag action">action: {l.action}</span>
                  <span className="evo-tag conf">conf {l.confidence.toFixed(2)}</span>
                  <span className={`evo-tag ${l.autoApproveEnabled ? 'aa-on' : 'aa-off'}`}>
                    {l.autoApproveEnabled ? '自动放行:开' : '自动放行:关'}
                  </span>
                  <button
                    className="evo-toggle"
                    disabled={busyId === l.id}
                    onClick={() => toggleAutoApprove(l.id, !l.autoApproveEnabled)}
                  >
                    {l.autoApproveEnabled ? '撤销' : '恢复'}
                  </button>
                </div>
                <div className="evo-evidence">
                  来源 Proposal：{l.proposalId} · run：
                  {l.runIds.length ? l.runIds.join(', ') : '—'}
                </div>
              </article>
            ))}
            <button className="reload" onClick={showAudit} style={{ marginTop: 8 }}>
              查看自动放行审计
            </button>
            {audits && (
              <div className="evo-audit">
                {audits.length === 0 ? (
                  <div className="empty">暂无自动放行审计</div>
                ) : (
                  audits.map((a) => (
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
                  ))
                )}
              </div>
            )}
          </section>
        </div>
      )}
    </div>
  )
}
