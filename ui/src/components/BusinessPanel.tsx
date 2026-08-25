import { useEffect, useState } from 'react'
import { Compass, Play, RefreshCw } from 'lucide-react'
import {
  getBusinessGoals,
  getBusinessPortfolio,
  getExpiredExecutionLeases,
  getBusinessFollowUps,
  getBusinessRecommendations,
  getBusinessLearningInsights,
  postProjectList,
  runBusinessActions,
  reconcileBusinessExecution,
  runBusinessNext,
  settleBusinessFollowUp,
  startBusinessFollowUp,
} from '../api'
import type {
  BusinessCandidateDto,
  BusinessGoalDto,
  BusinessPathDto,
  BusinessRecommendationDto,
  BusinessRunDto,
  ExecutionLeaseDto,
  FollowUpItemDto,
  LearningInsightDto,
  ProjectDto,
} from '../types'

function number(value: number): string {
  return value.toLocaleString('zh-CN', { maximumFractionDigits: 2 })
}

function CandidateCard({ item }: { item: BusinessCandidateDto }) {
  return (
    <div className="business-candidate">
      <b>{item.action}</b>
      <span className={item.netValue > 0 ? 'positive' : 'negative'}>
        净值 {number(item.netValue)}
      </span>
      <span>收益 {number(item.expectedBenefit)} / 成本 {number(item.cost)}</span>
      <span>置信 {(item.confidence * 100).toFixed(0)}%</span>
      <span>{item.actor === 'human' ? '人工' : '自动'}</span>
      <span>样本 {item.outcomeRuns} · 成功 {item.outcomeSuccesses}</span>
      {item.requiresApproval && <span className="approval">需审批</span>}
      {item.worker && <code>worker: {item.worker}</code>}
      {item.plugin && <code>plugin: {item.plugin}</code>}
      {(item.resources ? Object.entries(item.resources).length : 0) > 0 && (
        <span>
          资源：{Object.entries(item.resources ?? {}).map(([key, value]) => `${key} ${number(value)}`).join('，')}
        </span>
      )}
    </div>
  )
}

export function BusinessPanel({
  currentRole,
  onAuthError,
}: {
  currentRole: string
  onAuthError: (error: unknown) => boolean
}) {
  const [projects, setProjects] = useState<ProjectDto[]>([])
  const [projectId, setProjectId] = useState('')
  const [goals, setGoals] = useState<BusinessGoalDto[]>([])
  const [recommendations, setRecommendations] = useState<BusinessRecommendationDto[]>([])
  const [portfolio, setPortfolio] = useState<BusinessPathDto[]>([])
  const [interruptedLeases, setInterruptedLeases] = useState<ExecutionLeaseDto[]>([])
  const [followUps, setFollowUps] = useState<FollowUpItemDto[]>([])
  const [learningInsights, setLearningInsights] = useState<LearningInsightDto[]>([])
  const [run, setRun] = useState<BusinessRunDto | null>(null)
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const canRun = currentRole.toLowerCase() !== 'executive'

  const loadInsight = async (selectedId: string) => {
    const [
      goalResult,
      recommendationResult,
      portfolioResult,
      leaseResult,
      followUpResult,
      learningResult,
    ] = await Promise.allSettled([
      getBusinessGoals(selectedId),
      getBusinessRecommendations(selectedId),
      getBusinessPortfolio(selectedId),
      getExpiredExecutionLeases(selectedId),
      getBusinessFollowUps(selectedId),
      getBusinessLearningInsights(selectedId),
    ])
    setGoals(goalResult.status === 'fulfilled' ? goalResult.value : [])
    setRecommendations(recommendationResult.status === 'fulfilled' ? recommendationResult.value : [])
    setPortfolio(portfolioResult.status === 'fulfilled' ? portfolioResult.value : [])
    setInterruptedLeases(leaseResult.status === 'fulfilled' ? leaseResult.value : [])
    setFollowUps(followUpResult.status === 'fulfilled' ? followUpResult.value : [])
    setLearningInsights(learningResult.status === 'fulfilled' ? learningResult.value : [])
    const failures = [
      goalResult,
      recommendationResult,
      portfolioResult,
      leaseResult,
      followUpResult,
      learningResult,
    ].filter((result) => result.status === 'rejected')
    if (failures.length > 0) {
      throw (failures[0] as PromiseRejectedResult).reason
    }
  }

  const load = async () => {
    setLoading(true)
    setError(null)
    try {
      let list = projects
      if (!projectId) {
        list = await postProjectList()
        setProjects(list)
      }
      const selectedId = projectId || list[0]?.id || ''
      if (selectedId) {
        setProjectId(selectedId)
        await loadInsight(selectedId)
      }
    } catch (e) {
      if (!onAuthError(e)) setError((e as Error).message || '业务状态读取失败')
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    void load()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const changeProject = async (selectedId: string) => {
    setProjectId(selectedId)
    setRun(null)
    setLoading(true)
    setError(null)
    try {
      await loadInsight(selectedId)
    } catch (e) {
      if (!onAuthError(e)) setError((e as Error).message || '业务状态读取失败')
    } finally {
      setLoading(false)
    }
  }

  const execute = async () => {
    setBusy(true)
    setError(null)
    setNotice(null)
    try {
      const result = await runBusinessNext(projectId)
      setRun(result)
      const interrupted = result.expiredExecutionLeases?.length ?? 0
      setNotice(
        result.executed
          ? '已执行下一步最优动作'
          : interrupted > 0
            ? '发现中断执行，已安全停止；请人工对账'
            : '当前没有可自动执行的净收益动作',
      )
      await loadInsight(projectId)
    } catch (e) {
      if (!onAuthError(e)) setError((e as Error).message || '业务动作执行失败')
    } finally {
      setBusy(false)
    }
  }

  const reconcile = async (leaseId: string, opportunityId: string) => {
    setBusy(true)
    setError(null)
    setNotice(null)
    try {
      await reconcileBusinessExecution(projectId, opportunityId, leaseId)
      setNotice('已确认无副作用并重开机会')
      await loadInsight(projectId)
    } catch (e) {
      if (!onAuthError(e)) setError((e as Error).message || '中断执行处理失败')
    } finally {
      setBusy(false)
    }
  }

  const runBatch = async () => {
    setBusy(true)
    setError(null)
    setNotice(null)
    try {
      const runs = await runBusinessActions(projectId, 5)
      setRun(runs[runs.length - 1] ?? null)
      const executed = runs.filter((run) => run.executed).length
      setNotice(
        executed > 0
          ? `已连续执行 ${executed} 个安全动作`
          : '当前没有可自动执行的净收益动作',
      )
      await loadInsight(projectId)
    } catch (e) {
      if (!onAuthError(e)) setError((e as Error).message || '连续业务执行失败')
    } finally {
      setBusy(false)
    }
  }

  const updateFollowUp = async (operation: () => Promise<unknown>, message: string) => {
    setBusy(true)
    setError(null)
    setNotice(null)
    try {
      await operation()
      setNotice(message)
      await loadInsight(projectId)
    } catch (e) {
      if (!onAuthError(e)) setError((e as Error).message || '跟进验证失败')
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="knowledge-panel">
      <div className="evo-head">
        <h3><Compass size={16} /> Business · 企业闭环</h3>
        <button onClick={() => void load()} disabled={loading}>
          <RefreshCw size={13} /> {loading ? '加载中…' : '刷新'}
        </button>
      </div>
      <p className="evo-note">
        从目标缺口、机会推荐和资源约束组合中查看系统为什么选择某条路径；执行后可追踪决策与结果。
      </p>

      <section className="evo-section business-controls">
        <label>
          项目
          <select value={projectId} onChange={(event) => void changeProject(event.target.value)}>
            {projects.map((project) => (
              <option key={project.id} value={project.id}>{project.name || project.id}</option>
            ))}
          </select>
        </label>
        <button className="primary" disabled={!canRun || busy || !projectId} onClick={() => void execute()}>
          <Play size={13} /> {busy ? '执行中…' : '执行下一步'}
        </button>
        <button disabled={!canRun || busy || !projectId} onClick={() => void runBatch()}>
          执行至多 5 步
        </button>
      </section>

      {!canRun && <p className="evo-note">当前角色只读；Operator 及以上才能触发业务动作。</p>}
      {error && <div className="err-msg">⚠️ {error}</div>}
      {!error && notice && <div className="success-msg">✓ {notice}</div>}

      {loading && <div className="empty">正在读取企业状态…</div>}

      {!loading && goals.length > 0 && (
        <section className="evo-section">
          <h4>目标状态</h4>
          <div className="business-grid">
            {goals.map((goal) => (
              <article key={goal.id} className="knowledge-proposal">
                <div className="knowledge-proposal-head"><b>{goal.targetMetric}</b><small>{goal.status}</small></div>
                <div className="knowledge-meta">
                  <span>当前 {number(goal.currentValue)}</span>
                  <span>目标 {number(goal.targetValue)}</span>
                  <span>缺口 {number(goal.gap)}</span>
                </div>
              </article>
            ))}
          </div>
        </section>
      )}

      {!loading && interruptedLeases.length > 0 && (
        <section className="evo-section">
          <h4>中断恢复</h4>
          <div className="business-list">
            {interruptedLeases.map((lease) => (
              <article key={lease.id} className="knowledge-proposal">
                <div className="knowledge-proposal-head">
                  <b>中断执行 · {lease.opportunityId}</b>
                  <small>{lease.status}</small>
                </div>
                <div className="knowledge-meta">
                  <span>租约 {lease.id}</span>
                  <span>owner {lease.owner}</span>
                  <span>过期 {lease.expiresAt}</span>
                </div>
                <p>外部副作用未知；请先人工对账，再决定是否创建新机会。</p>
                <button disabled={!canRun || busy} onClick={() => void reconcile(lease.id, lease.opportunityId)}>
                  确认无副作用并重开
                </button>
              </article>
            ))}
          </div>
        </section>
      )}

      {!loading && followUps.length > 0 && (
        <section className="evo-section">
          <h4>验证队列</h4>
          <div className="business-list">
            {followUps.map((item) => (
              <article key={item.id} className="knowledge-proposal">
                <div className="knowledge-proposal-head">
                  <b>{item.subject || item.id} · {item.action ?? item.objectType}</b>
                  <small>{item.status}</small>
                </div>
                <div className="knowledge-meta">
                  <span>{item.objectType}</span>
                  {item.decisionId && <span>决策 {item.decisionId}</span>}
                </div>
                {item.status === 'Pending' ? (
                  <button
                    disabled={!canRun || busy}
                    onClick={() =>
                      void updateFollowUp(
                        () => startBusinessFollowUp(item.id),
                        '跟进已启动',
                      )
                    }
                  >
                    启动验证
                  </button>
                ) : (
                  <div className="business-controls">
                    <button
                      disabled={!canRun || busy}
                      onClick={() =>
                        void updateFollowUp(
                          () => settleBusinessFollowUp(item.id, true),
                          '已记录成功终局',
                        )
                      }
                    >
                      成功
                    </button>
                    <button
                      disabled={!canRun || busy}
                      onClick={() =>
                        void updateFollowUp(
                          () => settleBusinessFollowUp(item.id, false),
                          '已记录失败终局',
                        )
                      }
                    >
                      失败
                    </button>
                  </div>
                )}
              </article>
            ))}
          </div>
        </section>
      )}

      {!loading && learningInsights.length > 0 && (
        <section className="evo-section">
          <h4>学习证据</h4>
          <div className="business-grid">
            {learningInsights.map((item) => (
              <article key={`${item.opportunityType}:${item.action}`} className="knowledge-proposal">
                <div className="knowledge-proposal-head">
                  <b>{item.opportunityType} · {item.action}</b>
                  <small>样本 {item.outcomeRuns}</small>
                </div>
                <div className="knowledge-meta">
                  <span>成功 {item.outcomeSuccesses}</span>
                  <span>实际收益 {number(item.realizedGoalDelta)}</span>
                  <span>成功均值 {number(item.successfulAverageGoalDelta)}</span>
                </div>
              </article>
            ))}
          </div>
        </section>
      )}

      {!loading && recommendations.length > 0 && (
        <section className="evo-section">
          <h4>推荐与理由</h4>
          <div className="business-list">
            {recommendations.map((item) => (
              <article key={item.opportunityId + item.action} className="knowledge-proposal">
                <div className="knowledge-proposal-head">
                  <b>{item.subject} · {item.action}</b>
                  <small>评分 {number(item.score)}</small>
                </div>
                <p>{item.reason}</p>
                <div className="business-candidates">
                  {item.candidates.map((item) => <CandidateCard key={item.action} item={item} />)}
                </div>
              </article>
            ))}
          </div>
        </section>
      )}

      {!loading && portfolio.length > 0 && (
        <section className="evo-section">
          <h4>约束内组合</h4>
          <div className="business-list">
            {portfolio.map((path) => (
              <article key={path.opportunityId} className="knowledge-proposal">
                <div className="knowledge-proposal-head">
                  <b>{path.subject} · {path.actions.join(' → ')}</b>
                  <small>净值 {number(path.projectedNetValue)}</small>
                </div>
                <div className="knowledge-meta">
                  <span>成本 {number(path.projectedCost)}</span>
                  <span>置信 {(path.confidence * 100).toFixed(0)}%</span>
                  <span>路径样本 {path.outcomeRuns}</span>
                  <span>成功 {path.outcomeSuccesses}</span>
                  {path.goalMetric && <span>目标 {path.goalMetric}</span>}
                  {path.requiresHuman && <span>需要人工</span>}
                  {path.requiresApproval && <span>需要审批</span>}
                </div>
                <p>{path.reason}</p>
              </article>
            ))}
          </div>
        </section>
      )}

      {!loading && goals.length + recommendations.length + portfolio.length === 0 && !error && (
        <div className="empty">该项目暂无可读的业务目标或机会。</div>
      )}

      {run && (
        <section className="evo-section">
          <h4>最近执行</h4>
          {run.recommendation ? (
            <>
              <p><b>{run.recommendation.action}</b> — {run.recommendation.reason}</p>
              <details className="knowledge-justification">
                <summary>决策与执行证据</summary>
                <pre>{JSON.stringify({ decision: run.decision, execution: run.execution }, null, 2)}</pre>
              </details>
            </>
          ) : (
            <p>未选择动作。</p>
          )}
        </section>
      )}
    </div>
  )
}
