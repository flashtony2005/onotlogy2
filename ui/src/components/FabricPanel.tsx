//! Fabric 通信面板（Phase 6/7 UI）：Agent 拓扑 + fabric 事件流 + submit 表单。
//!
//! 数据源全部经 Facade 语义域（audit.query 读通信黑匣子；fabric.submit 提交）——
//! 单一语义源：本面板不直接访问 storage/kernel。
//! 拓扑 = audit fabric.agent_connected 去重（在线 Agent 及其最近连接）。

import { useEffect, useState } from 'react'
import { Network, Radio, Rocket, RefreshCw, Send, Users } from 'lucide-react'
import { postAuditQuery, postFabricSubmit } from '../api'
import { fabricEventLabel } from '../format'
import type { AuditEventDto, AuditViewDto, FabricSubmissionDto } from '../types'

const FABRIC_INTENTS: Record<string, string> = {
  modify_code: 'code.write',
  run_shell: 'shell.execute',
  run_tests: 'code.test',
}

export function FabricPanel() {
  const [audit, setAudit] = useState<AuditViewDto | null>(null)
  const [err, setErr] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  // submit 表单
  const [sessionId, setSessionId] = useState('sess_ui')
  const [sender, setSender] = useState('dsh')
  const [intent, setIntent] = useState('modify_code')
  const [payload, setPayload] = useState('{ "args": ["exec", "--skip-git-repo-check", "echo hello"], "object": "order-ui-1" }')
  const [submitting, setSubmitting] = useState(false)
  const [lastSubmit, setLastSubmit] = useState<FabricSubmissionDto | null>(null)
  const [submitErr, setSubmitErr] = useState<string | null>(null)

  const load = async () => {
    setLoading(true)
    setErr(null)
    try {
      const v = await postAuditQuery({ eventType: 'fabric.', limit: 200 })
      setAudit(v)
    } catch (e) {
      setErr(String(e))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    void load()
  }, [])

  const submit = async () => {
    setSubmitting(true)
    setSubmitErr(null)
    try {
      let parsed: unknown
      try {
        parsed = JSON.parse(payload)
      } catch (e) {
        throw new Error(`payload 不是合法 JSON: ${String(e)}`)
      }
      const sub = await postFabricSubmit({
        sessionId,
        sender,
        intent,
        payload: parsed,
      })
      setLastSubmit(sub)
      await load() // 提交即世界：刷新事件流（含 task_completed/failed）
    } catch (e) {
      setSubmitErr(String(e))
    } finally {
      setSubmitting(false)
    }
  }

  // Agent 拓扑：agent_connected 去重（按 refs[0]）
  const agents: { id: string; at: string }[] = []
  if (audit) {
    const seen = new Set<string>()
    for (const e of audit.events) {
      if (e.eventType === 'fabric.agent_connected') {
        const id = e.refs[0]
        if (!seen.has(id)) {
          seen.add(id)
          agents.push({ id, at: e.timestamp })
        }
      }
    }
  }
  const connectedCount = agents.length
  const fabricEvents = audit?.events ?? []

  return (
    <div className="world">
      <div className="dash-head">
        <div>
          <h3>通信织机（Fabric）</h3>
          <span className="dash-meta">
            Agent Communication Fabric · Agent {connectedCount} · 事件 {fabricEvents.length}
          </span>
        </div>
        <button className="reload" onClick={() => void load()} disabled={loading}>
          <RefreshCw size={13} /> {loading ? '加载中…' : '刷新'}
        </button>
      </div>

      {err && <div className="err-msg">⚠️ {err}</div>}
      {submitErr && <div className="err-msg">⚠️ {submitErr}</div>}

      <div className="world-cols">
        {/* 左列：Agent 拓扑 + submit 表单 */}
        <div className="fabric-col">
          <section className="dash-section">
            <h4>
              <Users size={14} /> Agent 拓扑（Capability Provider）
            </h4>
            {agents.length === 0 ? (
              <div className="empty-sm">暂无在线 Agent——适配器 connect 后出现在这里。</div>
            ) : (
              <div className="obj-grid">
                {agents.map((a) => (
                  <div key={a.id} className="obj-card">
                    <span className="obj-ic" style={{ color: '#3fae7f' }}>
                      <Radio size={18} strokeWidth={2} />
                    </span>
                    <span className="obj-label">{a.id}</span>
                    <span className="obj-count" style={{ fontSize: 11 }}>
                      在线
                    </span>
                  </div>
                ))}
              </div>
            )}
          </section>

          <section className="dash-section">
            <h4>
              <Rocket size={14} /> 提交任务（fabric.submit）
            </h4>
            <div className="fabric-form">
              <div className="world-filter">
                <input
                  placeholder="session_id"
                  value={sessionId}
                  onChange={(e) => setSessionId(e.target.value)}
                />
                <input
                  placeholder="sender"
                  value={sender}
                  onChange={(e) => setSender(e.target.value)}
                />
              </div>
              <div className="world-filter">
                <select value={intent} onChange={(e) => setIntent(e.target.value)}>
                  {Object.entries(FABRIC_INTENTS).map(([it, cap]) => (
                    <option key={it} value={it}>
                      {it} → {cap}
                    </option>
                  ))}
                </select>
              </div>
              <textarea
                rows={4}
                value={payload}
                onChange={(e) => setPayload(e.target.value)}
                placeholder='JSON 载荷：{ "args": [...], "object": "..." }'
              />
              <button
                className="reload"
                style={{ width: '100%', justifyContent: 'center' }}
                onClick={() => void submit()}
                disabled={submitting}
              >
                <Send size={13} /> {submitting ? '提交中…' : '提交'}
              </button>
              {lastSubmit && (
                <pre className="world-data">
                  {lastSubmit.result.sender} → {lastSubmit.result.receiver} ·{' '}
                  {String(lastSubmit.result.payload.status ?? '')} · corr{' '}
                  {lastSubmit.result.correlation_id ?? '-'}
                </pre>
              )}
            </div>
          </section>
        </div>

        {/* 右列：fabric 事件流（通信黑匣子） */}
        <div className="fabric-col">
          <section className="dash-section">
            <h4>
              <Network size={14} /> 通信事件流（fabric.* 黑匣子）
            </h4>
            {fabricEvents.length === 0 ? (
              <div className="empty-sm">暂无通信事件。</div>
            ) : (
              <div className="fabric-events">
                {fabricEvents.map((e: AuditEventDto) => (
                  <div key={e.id} className="fabric-ev">
                    <div className="fabric-ev-head">
                      <span className="fabric-ev-ty">{fabricEventLabel(e.eventType)}</span>
                      <code className="fabric-ev-refs">{e.refs.join(' → ')}</code>
                    </div>
                    <div className="fabric-ev-meta">
                      {e.eventType} · {e.timestamp}
                    </div>
                  </div>
                ))}
              </div>
            )}
          </section>
        </div>
      </div>
    </div>
  )
}
