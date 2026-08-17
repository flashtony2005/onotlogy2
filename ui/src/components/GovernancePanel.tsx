import { useState } from 'react'
import { ShieldCheck, RefreshCw, Check, X } from 'lucide-react'

// T1：人工裁决台——把挂起的 fabric/compose 审批从「curl/HTTP 端点」搬到 UI。
// 链路：GET /run/:run_id/approvals 拉挂起 → POST /run/:run_id/approval/:req_id 裁决 →
// 唤醒执行（approve）或终止（reject）。裁决事实由后端 fabric.approval_* 自动进审计轨。
import { getRunApprovals, postRunApprovalDecision } from '../api'
import type { PendingApprovalDto, ApprovalDecisionDto } from '../types'

export function GovernancePanel() {
  const [runId, setRunId] = useState('')
  const [items, setItems] = useState<PendingApprovalDto[]>([])
  const [loading, setLoading] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  const [feedback, setFeedback] = useState<string | null>(null)

  const load = async () => {
    const rid = runId.trim()
    if (!rid) {
      setErr('请填写 run_id（来自 fabric 提交的 correlation_id / trace_id）')
      return
    }
    setLoading(true)
    setErr(null)
    setFeedback(null)
    try {
      const list = await getRunApprovals(rid)
      setItems(list)
      if (list.length === 0) setFeedback('该 run 当前无挂起审批。')
    } catch (e) {
      setErr(String(e))
      setItems([])
    } finally {
      setLoading(false)
    }
  }

  // approve=true 放行续跑；approve=false 终止。裁决后从列表移除该项。
  const decide = async (reqId: string, approve: boolean) => {
    const rid = runId.trim()
    setErr(null)
    setFeedback(null)
    try {
      const r: ApprovalDecisionDto = await postRunApprovalDecision(rid, reqId, approve)
      setFeedback(
        `✅ ${approve ? '已批准' : '已拒绝'} ${reqId} → ${r.decision}` +
          (r.status ? ` · ${r.status}` : ''),
      )
      setItems((prev) => prev.filter((it) => it.reqId !== reqId))
    } catch (e) {
      setErr(String(e))
    }
  }

  return (
    <div className="evolution">
      <div className="evo-head">
        <h3>
          <ShieldCheck size={16} /> 治理 · 策略检查与审批裁决
        </h3>
      </div>
      <p className="evo-note">
        Learn≠Permission：知识不自动获得权限。提交 fabric 高危动作（require_approval）会挂起，
        运营人员在此 approve/reject；裁决经 <code>/run/:run_id/approval/:req_id</code> 唤醒执行或终止，
        事实由 <code>fabric.approval_*</code> 自动进审计轨。需 Operator 及以上角色。
      </p>

      <section className="dash-section">
        <h4>
          <ShieldCheck size={14} /> 挂起审批裁决（人工放行 → 执行 → 世界闭环）
        </h4>
        <div className="world-filter">
          <input
            style={{ flex: 1 }}
            placeholder="run_id（来自 fabric 提交的 correlation_id / trace_id）"
            value={runId}
            onChange={(e) => setRunId(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') void load()
            }}
          />
          <button onClick={() => void load()} disabled={loading}>
            <RefreshCw size={13} /> {loading ? '加载中…' : '加载挂起审批'}
          </button>
        </div>

        {err && <div className="err-msg">⚠️ {err}</div>}
        {feedback && (
          <div className="err-msg" style={{ color: '#7fe3c0', borderColor: 'rgba(127,227,192,0.4)', background: 'rgba(127,227,192,0.07)' }}>
            {feedback}
          </div>
        )}

        {!loading && items.length === 0 && !err && (
          <div className="empty-sm">该 run 当前无挂起审批。</div>
        )}

        <div className="fabric-events">
          {items.map((it) => (
            <div key={it.reqId} className="fabric-ev">
              <div className="fabric-ev-head">
                <span className="fabric-ev-ty">
                  {it.capability} · {it.action}
                </span>
                <code className="fabric-ev-refs">{it.reqId}</code>
              </div>
              <div className="fabric-ev-meta">
                {it.state}
                {it.args != null && (
                  <> · args: {truncate(JSON.stringify(it.args), 160)}</>
                )}
              </div>
              <div className="gov-actions">
                <button className="approve" onClick={() => void decide(it.reqId, true)}>
                  <Check size={13} /> 批准
                </button>
                <button className="reject" onClick={() => void decide(it.reqId, false)}>
                  <X size={13} /> 拒绝
                </button>
              </div>
            </div>
          ))}
        </div>
      </section>
    </div>
  )
}

function truncate(s: string, n: number): string {
  return s.length > n ? s.slice(0, n) + '…' : s
}
