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
      {/* 空间头部导航与操作 */}
      <div className="space-header">
        <div>
          <h2 className="space-title">
            <span className="space-icon">🎯</span> 业务大盘 · Business Dashboard
          </h2>
          <p className="space-subtitle">
            核心指标达成跟踪、智能体决策建议、高危动作人工审批与执行明细
          </p>
        </div>

        <button
          className="btn-action primary"
          onClick={handleAdvanceBusiness}
          disabled={advancing}
        >
          <RotateCcw size={14} className={advancing ? 'spin' : ''} />
          {advancing ? '分析测算中…' : '智能测算与优化'}
        </button>
      </div>

      {actionMsg && (
        <div className="space-banner">
          <Sparkles size={16} />
          <span>{actionMsg}</span>
        </div>
      )}

      {/* 如果有待审批项目，在顶部突出显示 */}
      {approvals.length > 0 && (
        <div className="glass-card" style={{ borderColor: 'var(--warning)', background: 'var(--warning-light)', marginBottom: '16px' }}>
          <div className="card-header-row">
            <h3 className="card-title" style={{ color: '#b45309' }}>
              <AlertTriangle size={18} /> 待人工审批放行 ({approvals.length} 项高风险操作)
            </h3>
            <span style={{ fontSize: '12px', color: '#b45309' }}>需人工授权后智能体方可继续</span>
          </div>
          <div className="approvals-list">
            {approvals.map((appr) => (
              <div key={appr.reqId} className="approval-card" style={{ background: '#ffffff' }}>
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
        </div>
      )}

      {/* 主栅格：第一行 - 核心指标 与 智能推荐 */}
      <div className="cockpit-grid" style={{ marginBottom: '20px' }}>
        {/* 核心指标 */}
        <div className="glass-card">
          <div className="card-header-row">
            <h3 className="card-title">
              <Layers size={17} /> 核心业务指标跟踪 (Goals & Metrics)
            </h3>
            <span style={{ fontSize: '12px', color: 'var(--text-muted)' }}>共 {goals.length} 项监控指标</span>
          </div>

          <div className="goals-list">
            {goals.map((g) => (
              <div key={g.id} className="goal-item">
                <div className="goal-info">
                  <span className="goal-name">{g.targetMetric}</span>
                  <span className={`goal-status status-${g.status.toLowerCase()}`}>
                    <span className="status-dot" />
                    {g.status === 'IN_PROGRESS'
                      ? '进行中'
                      : g.status === 'COMPLIANT'
                      ? '已达标'
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

        {/* 智能推荐方案 */}
        <div className="glass-card">
          <div className="card-header-row">
            <h3 className="card-title">
              <Sparkles size={17} /> 智能推荐行动方案 (Recommended Actions)
            </h3>
            <span style={{ fontSize: '12px', color: 'var(--text-muted)' }}>AI 研判建议</span>
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

      {/* 主栅格：第二行 - 执行时序与决策结论 与 安全合规日志 */}
      <div className="cockpit-grid">
        {/* 执行时序与决策 */}
        <div className="glass-card">
          <div className="card-header-row">
            <h3 className="card-title">
              <Cpu size={17} /> 任务执行流水与结论 (Run Activity)
            </h3>
            {runId && (
              <span className="run-id-tag" style={{ fontSize: '11px' }}>
                任务编号: <code>{runId.slice(-6)}</code>
              </span>
            )}
          </div>

          {timeline.decision && (
            <div className="decision-banner" style={{ marginBottom: '14px' }}>
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
            <div className="empty-state" style={{ padding: '24px 0' }}>
              <Cpu size={28} />
              <h4>暂无运行记录</h4>
              <p>在底部输入框输入任务目标后按回车，或在上方点击「一键执行此方案」开始。</p>
            </div>
          ) : (
            <div className="events-stream" style={{ maxHeight: '280px', overflowY: 'auto' }}>
              {events.map((ev, idx) => (
                <div key={ev.id || idx} className={`stream-card step-${ev.eventType.toLowerCase()}`}>
                  <div className="stream-card-head">
                    <span className="stream-badge">{ev.eventType}</span>
                    <span className="stream-time">{fmtTime(ev.timestamp)}</span>
                    {ev.confidence != null && (
                      <span className="stream-conf">可信度 {(ev.confidence * 100).toFixed(0)}%</span>
                    )}
                  </div>
                  {ev.reasoning && <p className="stream-reasoning">{ev.reasoning}</p>}
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

        {/* 系统安全审计日志 */}
        <div className="glass-card">
          <div className="card-header-row">
            <h3 className="card-title">
              <CheckCircle2 size={17} /> 系统操作与安全审计 (Audit Logs)
            </h3>
            <span style={{ fontSize: '12px', color: 'var(--success)', fontWeight: 600 }}>
              ● 安全合规运行中
            </span>
          </div>

          <div className="audit-timeline" style={{ maxHeight: '340px', overflowY: 'auto' }}>
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
    </div>
  )
}
