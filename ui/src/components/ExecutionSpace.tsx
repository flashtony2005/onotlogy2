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

      {/* 如果有待审批项目，在顶部突出显示：以本体论业务操作凭据展示，拒绝技术暗语 */}
      {approvals.length > 0 && (
        <div className="glass-card" style={{ borderColor: '#d97706', background: 'rgba(254, 243, 199, 0.35)', marginBottom: '24px', boxShadow: '0 4px 20px rgba(217, 119, 6, 0.08)' }}>
          <div className="card-header-row" style={{ borderBottom: '1px solid rgba(217, 119, 6, 0.2)', paddingBottom: '12px', marginBottom: '16px' }}>
            <div>
              <h3 className="card-title" style={{ color: '#92400e', fontSize: '16px', display: 'flex', alignItems: 'center', gap: '8px' }}>
                <AlertTriangle size={20} color="#d97706" /> 待人工形式化裁决放行 ({approvals.length} 项关键业务操作)
              </h3>
              <p style={{ margin: '4px 0 0 0', fontSize: '13px', color: '#b45309' }}>
                触碰治理红线或超出自动化预算限额。需由业务负责人核验本体上下文、形式化证明与预期损益后签署。
              </p>
            </div>
            <span style={{ fontSize: '12px', background: '#fef3c7', color: '#92400e', padding: '4px 10px', borderRadius: '12px', fontWeight: 600, border: '1px solid #fde68a' }}>
              本体状态冻结中
            </span>
          </div>

          <div className="approvals-list" style={{ display: 'flex', flexDirection: 'column', gap: '20px' }}>
            {approvals.map((appr) => (
              <div key={appr.reqId} className="ontology-approval-card">
                {/* 1. 卡片顶栏：单号、目标业务实体、风险标签 */}
                <div className="ont-appr-header">
                  <div className="ont-appr-meta">
                    <span className="ont-appr-id">审批凭据 #{appr.reqId}</span>
                    {appr.targetEntity ? (
                      <span className="ont-entity-pill" style={{ borderColor: appr.targetEntity.badgeColor || '#059669' }}>
                        <span className="dot" style={{ background: appr.targetEntity.badgeColor || '#059669' }} />
                        <span className="type">{appr.targetEntity.type}:</span>
                        <strong className="name">{appr.targetEntity.name}</strong>
                      </span>
                    ) : (
                      <span className="ont-entity-pill">
                        <span className="type">系统能力:</span>
                        <strong className="name">{appr.capability}</strong>
                      </span>
                    )}
                  </div>
                  {appr.businessImpact && (
                    <span className={`ont-risk-badge risk-${appr.businessImpact.riskLevel === '高风险' ? 'high' : appr.businessImpact.riskLevel === '中风险' ? 'medium' : 'low'}`}>
                      {appr.businessImpact.riskLevel}
                    </span>
                  )}
                </div>

                {/* 2. 核心业务意图标题（白话人话直击目标） */}
                <h4 className="ont-appr-title">
                  {appr.intentTitle || appr.action}
                </h4>

                {/* 3. 业务归因与触发背景 */}
                {appr.businessAttribution && (
                  <div className="ont-appr-block ont-bg-reason">
                    <span className="block-label">📌 业务归因与触发背景</span>
                    <p className="block-text">{appr.businessAttribution}</p>
                  </div>
                )}

                {/* 4. 形式化证明与红线规则约束 (Formal Invariant Guard) */}
                {appr.formalInvariant && (
                  <div className="ont-appr-block ont-formal-proof">
                    <div className="proof-header">
                      <span className="block-label" style={{ color: '#92400e' }}>
                        ⚖️ 形式化约束与红线证明 ({appr.formalInvariant.ruleId} · {appr.formalInvariant.ruleName})
                      </span>
                      <span className="proof-badge">不可变式红线判定</span>
                    </div>
                    <div className="proof-formula-box">
                      <code>{appr.formalInvariant.formula}</code>
                    </div>
                    <p className="proof-desc">
                      <strong>形式证明判定：</strong>{appr.formalInvariant.proofConclusion}
                    </p>
                    <p className="proof-reason">
                      <strong>裁决触发原因：</strong>{appr.formalInvariant.requiresHumanReviewReason}
                    </p>
                  </div>
                )}

                {/* 5. 参数人话白话对照表 (取代生硬不可读的 JSON) */}
                {appr.paramTranslations && appr.paramTranslations.length > 0 ? (
                  <div className="ont-appr-block ont-params-block">
                    <span className="block-label">📋 参数意图白话对照表 (取代底层技术代码)</span>
                    <div className="param-table-wrap">
                      <table className="ont-param-table">
                        <thead>
                          <tr>
                            <th>业务参数</th>
                            <th>拟定取值 (变更前后)</th>
                            <th>参数作用与业务影响</th>
                          </tr>
                        </thead>
                        <tbody>
                          {appr.paramTranslations.map((pt, idx) => (
                            <tr key={idx}>
                              <td className="param-label-cell">
                                <span className="param-name">{pt.label}</span>
                                <code className="param-code">({pt.param})</code>
                              </td>
                              <td className="param-val-cell">
                                {pt.oldValue && (
                                  <div className="val-old">
                                    <span className="val-tag">原值:</span> {pt.oldValue}
                                  </div>
                                )}
                                <div className="val-new">
                                  <span className="val-tag">变更为:</span> <strong>{pt.newValue}</strong>
                                </div>
                              </td>
                              <td className="param-desc-cell">{pt.explanation}</td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  </div>
                ) : (
                  <div className="ont-appr-block">
                    <span className="block-label">📋 操作参数</span>
                    <pre className="appr-args">{fmtJson(appr.args)}</pre>
                  </div>
                )}

                {/* 6. 预期业务损益与风险控制边界 */}
                {appr.businessImpact && (
                  <div className="ont-impact-grid">
                    <div className="impact-card gain">
                      <span className="impact-title">🎯 预期业务收益</span>
                      <span className="impact-val">{appr.businessImpact.expectedGain}</span>
                    </div>
                    <div className="impact-card cost">
                      <span className="impact-title">💰 财务/算力成本</span>
                      <span className="impact-val">{appr.businessImpact.budgetCost}</span>
                    </div>
                    <div className="impact-card roi">
                      <span className="impact-title">📈 投产比 (ROI)</span>
                      <span className="impact-val">{appr.businessImpact.roiSummary}</span>
                    </div>
                    <div className="impact-card defense">
                      <span className="impact-title">🛡️ 风险防御与回滚</span>
                      <span className="impact-val">{appr.businessImpact.riskMitigation}</span>
                    </div>
                  </div>
                )}

                {/* 7. 底层技术审计透视 (可折叠) */}
                <details className="ont-tech-details">
                  <summary className="tech-summary">
                    <span>🔍 查看底层接口技术调用明细 ({appr.capability} / {appr.action})</span>
                  </summary>
                  <div className="tech-payload">
                    <div style={{ marginBottom: '6px', fontSize: '11px', color: 'var(--text-muted)' }}>
                      智能体底层 RPC 调用载荷（原始状态）:
                    </div>
                    <pre>{fmtJson(appr.args)}</pre>
                  </div>
                </details>

                {/* 8. 裁决操作按钮栏 */}
                <div className="ont-decision-footer">
                  <div className="decision-tip">
                    <span>签署后将即时完成本体状态变迁，并计入全局防篡改审计轨</span>
                  </div>
                  <div className="decision-btns">
                    <button
                      className="btn-appr approve"
                      onClick={() => handleDecision(appr.reqId, true)}
                    >
                      <CheckCircle2 size={16} /> 批准放行并应用本体变更
                    </button>
                    <button
                      className="btn-appr reject"
                      onClick={() => handleDecision(appr.reqId, false)}
                    >
                      <XCircle size={16} /> 驳回并要求智能体重新规划
                    </button>
                  </div>
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
