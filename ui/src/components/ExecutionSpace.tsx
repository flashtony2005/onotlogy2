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
      setActionMsg('已完成智能分析与指标测算，推荐方案已更新')
      setTimeout(() => setActionMsg(null), 3500)
    } finally {
      setAdvancing(false)
    }
  }

  const handleDecision = async (reqId: string, approve: boolean) => {
    try {
      await postRunApprovalDecision('', reqId, approve)
      await loadData()
      setActionMsg(approve ? '已批准操作，任务已继续执行' : '已拒绝该操作，终止执行')
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
            <span className="space-icon">🎯</span> 业务任务中心 · Tasks & Execution
          </h2>
          <p className="space-subtitle">
            跟踪核心业务指标达成、查看智能体自动化执行过程与处理安全审批
          </p>
        </div>

        <div className="space-subnav">
          <button
            className={`space-tab-btn ${subTab === 'cockpit' ? 'active' : ''}`}
            onClick={() => setSubTab('cockpit')}
          >
            业务看板 ({goals.length} 项指标)
          </button>
          <button
            className={`space-tab-btn ${subTab === 'timeline' ? 'active' : ''}`}
            onClick={() => setSubTab('timeline')}
          >
            运行明细 {runId ? `(#${runId.slice(-4)})` : ''}
          </button>
          <button
            className={`space-tab-btn ${subTab === 'governance' ? 'active' : ''}`}
            onClick={() => setSubTab('governance')}
          >
            安全审批 {approvals.length > 0 ? `(${approvals.length} 待审批)` : ''}
          </button>
        </div>
      </div>

      {actionMsg && (
        <div className="space-banner">
          <Sparkles size={16} />
          <span>{actionMsg}</span>
        </div>
      )}

      {/* ── 视图 1：业务看板 (Cockpit) ─────────────────────────────────── */}
      {subTab === 'cockpit' && (
        <div className="cockpit-grid">
          {/* 目标与指标缺口 */}
          <div className="glass-card">
            <div className="card-header-row">
              <h3 className="card-title">
                <Layers size={17} /> 核心业务指标跟踪 (Goals & Metrics)
              </h3>
              <button
                className="btn-action primary"
                onClick={handleAdvanceBusiness}
                disabled={advancing}
              >
                <RotateCcw size={14} className={advancing ? 'spin' : ''} />
                {advancing ? '分析测算中…' : '智能测算与优化'}
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
                      <span className="label">当前实际值</span>
                      <span className="val curr">{g.currentValue}</span>
                    </div>
                    <ArrowRight size={14} className="arrow" />
                    <div className="metric-box">
                      <span className="label">预期目标值</span>
                      <span className="val target">{g.targetValue}</span>
                    </div>
                    <div className="metric-box">
                      <span className="label">距离目标差距</span>
                      <span className="val gap">
                        {g.gap > 0 ? `-${g.gap}` : '已达成'}
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
                <Sparkles size={17} /> 智能推荐行动方案 (Recommended Actions)
              </h3>
            </div>

            <div className="rec-list">
              {recommendations.map((rec) => (
                <div key={rec.opportunityId} className="rec-card">
                  <div className="rec-top">
                    <span className="rec-subject">{rec.subject}</span>
                    <span className="rec-score">可信度 {(rec.confidence * 100).toFixed(0)}%</span>
                  </div>
                  <h4 className="rec-action">{rec.action}</h4>
                  <p className="rec-reason">{rec.reason}</p>

                  <div className="rec-footer">
                    <span className="rec-actor">执行方式: {rec.actor === 'agent' ? '🤖 智能体自动执行' : '👤 人工协助跟进'}</span>
                    <button
                      className="btn-action secondary"
                      onClick={() => onTriggerRun(`执行针对 ${rec.subject} 的策略: ${rec.action}`)}
                      disabled={running}
                    >
                      <Play size={13} /> 一键执行此方案
                    </button>
                  </div>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}

      {/* ── 视图 2：运行明细 (Timeline) ─────────────────────────────────── */}
      {subTab === 'timeline' && (
        <div className="timeline-container">
          {runId && (
            <div className="run-status-card">
              <div className="run-meta-row">
                <span className="run-id-tag">任务编号: <code>{runId}</code></span>
                <span className={`run-badge status-${timeline.status.toLowerCase()}`}>
                  {timeline.status}
                </span>
                <span className="run-approval-tag">
                  安全审核: <b>{timeline.humanApproval === 'Approved' ? '已放行' : timeline.humanApproval}</b>
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
                <span className="lbl">任务目标</span>
                <p>{timeline.decision.mission}</p>
              </div>
              <div className="dec-item">
                <span className="lbl">执行结论</span>
                <p className="hl">{timeline.decision.decision}</p>
              </div>
              <div className="dec-item">
                <span className="lbl">风险评估</span>
                <p>{timeline.decision.risk}</p>
              </div>
            </div>
          )}

          {events.length === 0 ? (
            <div className="empty-state">
              <Cpu size={32} />
              <h4>暂无运行记录</h4>
              <p>在底部输入框输入任务目标后按回车，或在业务看板点击「一键执行此方案」开始。</p>
            </div>
          ) : (
            <div className="events-stream">
              {events.map((ev, idx) => (
                <div key={ev.id || idx} className={`stream-card step-${ev.eventType.toLowerCase()}`}>
                  <div className="stream-card-head">
                    <span className="stream-badge">{ev.eventType}</span>
                    <span className="stream-time">{fmtTime(ev.timestamp)}</span>
                    {ev.confidence != null && (
                      <span className="stream-conf">可信度 {(ev.confidence * 100).toFixed(0)}%</span>
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
                      <summary>输出结果</summary>
                      <pre>{fmtJson(ev.output)}</pre>
                    </details>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {/* ── 视图 3：安全审批 (Governance) ─────────────────────────────── */}
      {subTab === 'governance' && (
        <div className="governance-grid">
          <div className="glass-card">
            <h3 className="card-title">
              <AlertTriangle size={17} /> 待人工审批的高风险操作 (Pending Approvals)
            </h3>
            <p className="card-desc">
              当智能体执行涉及大额优惠、权限变更或高敏感操作时，将自动暂停并等待人工审批放行。
            </p>

            {approvals.length === 0 ? (
              <div className="clean-state">
                <CheckCircle2 size={28} className="clean-icon" />
                <p>当前各项操作均符合安全规范，暂无待审批事项。</p>
              </div>
            ) : (
              <div className="approvals-list">
                {approvals.map((appr) => (
                  <div key={appr.reqId} className="approval-card">
                    <div className="appr-head">
                      <span className="appr-id">审批单号 #{appr.reqId}</span>
                      <span className="appr-cap">{appr.capability}</span>
                    </div>
                    <h4 className="appr-action">{appr.action}</h4>
                    <pre className="appr-args">{fmtJson(appr.args)}</pre>

                    <div className="appr-actions">
                      <button
                        className="btn-appr approve"
                        onClick={() => handleDecision(appr.reqId, true)}
                      >
                        <CheckCircle2 size={15} /> 批准执行
                      </button>
                      <button
                        className="btn-appr reject"
                        onClick={() => handleDecision(appr.reqId, false)}
                      >
                        <XCircle size={15} /> 拒绝操作
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>

          <div className="glass-card">
            <h3 className="card-title">
              <CheckCircle2 size={17} /> 系统操作审计日志 (Audit Logs)
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
