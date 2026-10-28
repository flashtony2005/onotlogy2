import { useState, useEffect } from 'react'
import {
  Play,
  RotateCcw,
  CheckCircle2,
  XCircle,
  AlertTriangle,
  ArrowRight,
  Cpu,
  Layers,
  Sparkles,
} from 'lucide-react'
import {
  getBusinessGoals,
  getBusinessRecommendations,
  runBusinessNext,
  getRunApprovals,
  postRunApprovalDecision,
  postAuditQuery,
} from '../api'
import type {
  BusinessGoalDto,
  BusinessRecommendationDto,
  PendingApprovalDto,
  TraceEventDto,
  RunTimeline,
} from '../types'
import { fmtTime, fmtJson } from '../format'

interface ExecutionSpaceProps {
  currentRole: string
  runId: string | null
  timeline: RunTimeline
  events: TraceEventDto[]
  running: boolean
  onTriggerRun: (goal: string) => void
}

export function ExecutionSpace({
  currentRole: _currentRole,
  runId,
  timeline,
  events,
  running,
  onTriggerRun,
}: ExecutionSpaceProps) {
  const [subTab, setSubTab] = useState<'cockpit' | 'timeline' | 'governance'>('cockpit')
  const [goals, setGoals] = useState<BusinessGoalDto[]>([])
  const [recommendations, setRecommendations] = useState<BusinessRecommendationDto[]>([])
  const [approvals, setApprovals] = useState<PendingApprovalDto[]>([])
  const [auditLogs, setAuditLogs] = useState<{ id: string; eventType: string; timestamp: string; payload: unknown }[]>([])
  const [advancing, setAdvancing] = useState(false)
  const [actionMsg, setActionMsg] = useState<string | null>(null)

  const loadData = async () => {
    try {
      const [g, r, a, logs] = await Promise.all([
        getBusinessGoals('proj-crm-retention'),
        getBusinessRecommendations('proj-crm-retention'),
        getRunApprovals(''),
        postAuditQuery({ limit: 10 }),
      ])
      setGoals(g)
      setRecommendations(r)
      setApprovals(a)
      setAuditLogs(logs.events as any[])
    } catch {
      /* ignore */
    }
  }

  useEffect(() => {
    loadData()
  }, [])

  const handleAdvanceBusiness = async () => {
    setAdvancing(true)
    try {
      await runBusinessNext('proj-crm-retention')
      await loadData()
      setActionMsg('已触发业务闭环寻优演进，目标差距已收敛')
      setTimeout(() => setActionMsg(null), 3500)
    } finally {
      setAdvancing(false)
    }
  }

  const handleDecision = async (reqId: string, approve: boolean) => {
    try {
      await postRunApprovalDecision('', reqId, approve)
      await loadData()
      setActionMsg(approve ? '已批准放行，解除执行阻断' : '已拒绝该请求，终止高危动作')
      setTimeout(() => setActionMsg(null), 3500)
    } catch (e) {
      setActionMsg(`审批处理失败: ${(e as Error).message}`)
    }
  }

  return (
    <div className="space-root">
      {/* 空间头部导航与快速统计 */}
      <div className="space-header">
        <div>
          <h2 className="space-title">
            <span className="space-icon">🎯</span> 运行与决策空间 · Execution & Decisions
          </h2>
          <p className="space-subtitle">
            连接现实指标缺口、智能体认知推理时间线与人类治理闸门
          </p>
        </div>

        <div className="space-subnav">
          <button
            className={`space-tab-btn ${subTab === 'cockpit' ? 'active' : ''}`}
            onClick={() => setSubTab('cockpit')}
          >
            业务驾驶舱 ({goals.length} 目标)
          </button>
          <button
            className={`space-tab-btn ${subTab === 'timeline' ? 'active' : ''}`}
            onClick={() => setSubTab('timeline')}
          >
            认知流水线 {runId ? `(Run #${runId.slice(-4)})` : ''}
          </button>
          <button
            className={`space-tab-btn ${subTab === 'governance' ? 'active' : ''}`}
            onClick={() => setSubTab('governance')}
          >
            治理审批 {approvals.length > 0 ? `(${approvals.length} 待决)` : ''}
          </button>
        </div>
      </div>

      {actionMsg && (
        <div className="space-banner">
          <Sparkles size={16} />
          <span>{actionMsg}</span>
        </div>
      )}

      {/* ── 视图 1：业务驾驶舱 (Cockpit) ─────────────────────────────────── */}
      {subTab === 'cockpit' && (
        <div className="cockpit-grid">
          {/* 目标与指标缺口 */}
          <div className="glass-card">
            <div className="card-header-row">
              <h3 className="card-title">
                <Layers size={17} /> 企业目标差距追踪 (Goal Gap)
              </h3>
              <button
                className="btn-action primary"
                onClick={handleAdvanceBusiness}
                disabled={advancing}
              >
                <RotateCcw size={14} className={advancing ? 'spin' : ''} />
                {advancing ? '寻优计算中…' : '推进一轮优化'}
              </button>
            </div>

            <div className="goals-list">
              {goals.map((g) => (
                <div key={g.id} className="goal-item">
                  <div className="goal-info">
                    <span className="goal-name">{g.targetMetric}</span>
                    <span className={`goal-status status-${g.status.toLowerCase()}`}>
                      <span className="status-dot" />
                      {g.status === 'IN_PROGRESS'
                        ? '进行中 · In Progress'
                        : g.status === 'COMPLIANT'
                        ? '已达标 · Compliant'
                        : g.status}
                    </span>
                  </div>
                  <div className="goal-metric-row">
                    <div className="metric-box">
                      <span className="label">当前值</span>
                      <span className="val curr">{g.currentValue}</span>
                    </div>
                    <ArrowRight size={14} className="arrow" />
                    <div className="metric-box">
                      <span className="label">目标值</span>
                      <span className="val target">{g.targetValue}</span>
                    </div>
                    <div className="metric-box">
                      <span className="label">剩余缺口</span>
                      <span className="val gap">
                        {g.gap > 0 ? `-${g.gap}` : '已达标'}
                      </span>
                    </div>
                  </div>
                  <div className="goal-progress-bar">
                    <div
                      className="goal-fill"
                      style={{
                        width: `${Math.min(
                          100,
                          g.targetValue > 0
                            ? Math.max(10, ((g.currentValue / g.targetValue) * 100))
                            : 100
                        )}%`,
                      }}
                    />
                  </div>
                </div>
              ))}
            </div>
          </div>

          {/* 机会与干预推荐 */}
          <div className="glass-card">
            <div className="card-header-row">
              <h3 className="card-title">
                <Sparkles size={17} /> 机会发现与策略推荐 (Recommendations)
              </h3>
            </div>

            <div className="rec-list">
              {recommendations.map((rec) => (
                <div key={rec.opportunityId} className="rec-card">
                  <div className="rec-top">
                    <span className="rec-subject">{rec.subject}</span>
                    <span className="rec-score">置信度 {(rec.confidence * 100).toFixed(0)}%</span>
                  </div>
                  <h4 className="rec-action">{rec.action}</h4>
                  <p className="rec-reason">{rec.reason}</p>

                  <div className="rec-footer">
                    <span className="rec-actor">执行主体: {rec.actor === 'agent' ? '🤖 智能体' : '👤 人工协同'}</span>
                    <button
                      className="btn-action secondary"
                      onClick={() => onTriggerRun(`执行针对 ${rec.subject} 的策略: ${rec.action}`)}
                      disabled={running}
                    >
                      <Play size={13} /> 立即驱动 Agent 执行
                    </button>
                  </div>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}

      {/* ── 视图 2：认知流水线 (Timeline) ─────────────────────────────────── */}
      {subTab === 'timeline' && (
        <div className="timeline-container">
          {runId && (
            <div className="run-status-card">
              <div className="run-meta-row">
                <span className="run-id-tag">Run: <code>{runId}</code></span>
                <span className={`run-badge status-${timeline.status.toLowerCase()}`}>
                  {timeline.status}
                </span>
                <span className="run-approval-tag">
                  治理闸门: <b>{timeline.humanApproval}</b>
                </span>
              </div>
              <div className="run-progress-track">
                <div className="run-progress-fill" style={{ width: `${timeline.progress}%` }} />
              </div>
            </div>
          )}

          {timeline.decision && (
            <div className="decision-banner">
              <div className="dec-item">
                <span className="lbl">使命目标</span>
                <p>{timeline.decision.mission}</p>
              </div>
              <div className="dec-item">
                <span className="lbl">决策结论</span>
                <p className="hl">{timeline.decision.decision}</p>
              </div>
              <div className="dec-item">
                <span className="lbl">风控研判</span>
                <p>{timeline.decision.risk}</p>
              </div>
            </div>
          )}

          {events.length === 0 ? (
            <div className="empty-state">
              <Cpu size={32} />
              <h4>等待认知触发</h4>
              <p>在底部对话条输入目标并点击「▶」触发实时认知闭环推理。</p>
            </div>
          ) : (
            <div className="events-stream">
              {events.map((ev, idx) => (
                <div key={ev.id || idx} className={`stream-card step-${ev.eventType.toLowerCase()}`}>
                  <div className="stream-card-head">
                    <span className="stream-badge">{ev.eventType}</span>
                    <span className="stream-time">{fmtTime(ev.timestamp)}</span>
                    {ev.confidence != null && (
                      <span className="stream-conf">置信度 {(ev.confidence * 100).toFixed(0)}%</span>
                    )}
                  </div>
                  {ev.objectRefs && ev.objectRefs.length > 0 && (
                    <div className="stream-refs">
                      {ev.objectRefs.map((ref) => (
                        <span key={ref} className="ref-chip">
                          {ref}
                        </span>
                      ))}
                    </div>
                  )}
                  {ev.reasoning && <p className="stream-reasoning">{ev.reasoning}</p>}
                  {ev.input != null && (
                    <details className="stream-details">
                      <summary>输入参数</summary>
                      <pre>{fmtJson(ev.input)}</pre>
                    </details>
                  )}
                  {ev.output != null && (
                    <details className="stream-details">
                      <summary>执行产出</summary>
                      <pre>{fmtJson(ev.output)}</pre>
                    </details>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {/* ── 视图 3：治理审批闸门 (Governance) ─────────────────────────────── */}
      {subTab === 'governance' && (
        <div className="governance-grid">
          <div className="glass-card">
            <h3 className="card-title">
              <AlertTriangle size={17} /> 挂起的人工放行申请 (Pending Approvals)
            </h3>
            <p className="card-desc">
              当策略触发高风险阈值（如大额现金流折算或参数变更）时，系统进入安全等待状态。
            </p>

            {approvals.length === 0 ? (
              <div className="clean-state">
                <CheckCircle2 size={28} className="clean-icon" />
                <p>当前所有规则合规，无挂起的审批事项。</p>
              </div>
            ) : (
              <div className="approvals-list">
                {approvals.map((appr) => (
                  <div key={appr.reqId} className="approval-card">
                    <div className="appr-head">
                      <span className="appr-id">Req #{appr.reqId}</span>
                      <span className="appr-cap">{appr.capability}</span>
                    </div>
                    <h4 className="appr-action">{appr.action}</h4>
                    <pre className="appr-args">{fmtJson(appr.args)}</pre>

                    <div className="appr-actions">
                      <button
                        className="btn-appr approve"
                        onClick={() => handleDecision(appr.reqId, true)}
                      >
                        <CheckCircle2 size={15} /> 批准放行
                      </button>
                      <button
                        className="btn-appr reject"
                        onClick={() => handleDecision(appr.reqId, false)}
                      >
                        <XCircle size={15} /> 否决终止
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>

          <div className="glass-card">
            <h3 className="card-title">
              <CheckCircle2 size={17} /> 审计追踪记录 (Audit Trail)
            </h3>
            <div className="audit-timeline">
              {auditLogs.map((log) => (
                <div key={log.id} className="audit-row">
                  <span className="audit-time">{fmtTime(log.timestamp)}</span>
                  <span className="audit-type">{log.eventType}</span>
                  <span className="audit-payload">{fmtJson(log.payload)}</span>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
