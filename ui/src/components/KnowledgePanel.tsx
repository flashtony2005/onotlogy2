import { useEffect, useState } from 'react'
import { BookOpen, GitBranch, RefreshCw } from 'lucide-react'
import {
  applyKnowledgeProposal,
  createKnowledgeProposal,
  diagnoseKnowledge,
  getKnowledgeCurrent,
  getKnowledgeProposals,
  getKnowledgeVersions,
  proposeKnowledgeRollback,
  reviewKnowledgeProposal,
} from '../api'
import {
  knowledgeHealthLabel,
  knowledgeNextVersionId,
  knowledgeProposalStage,
  parseKnowledgeProposalDraft,
} from '../knowledge'
import { fmtJson, fmtTime } from '../format'
import type {
  KnowledgeDiagnosisDto,
  KnowledgeVersionDto,
  OntologyEvolutionRecordDto,
} from '../types'

const stageLabels: Record<string, string> = {
  pending: '待评审',
  approved: '已批准',
  rejected: '已拒绝',
  applied: '已应用',
}

async function loadGovernance() {
  const [versions, proposals] = await Promise.all([
    getKnowledgeVersions(),
    getKnowledgeProposals(),
  ])
  return { versions, proposals }
}

function KnowledgeProposalCard({
  proposal,
  busy,
  defaultTargetId,
  onReview,
  onApply,
}: {
  proposal: OntologyEvolutionRecordDto
  busy: boolean
  defaultTargetId: string
  onReview: (id: string, approve: boolean, reason?: string) => Promise<void>
  onApply: (id: string, targetVersionId: string) => Promise<void>
}) {
  const [reason, setReason] = useState('')
  const [targetId, setTargetId] = useState(defaultTargetId)
  const stage = knowledgeProposalStage(proposal)

  return (
    <article className="knowledge-proposal">
      <div className="knowledge-proposal-head">
        <b>{proposal.id}</b>
        <span className={`knowledge-stage ${stage}`}>{stageLabels[stage]}</span>
      </div>
      <div className="knowledge-meta">
        <span>source {proposal.source_version_id}</span>
        {proposal.applied_version_id && <span>applied {proposal.applied_version_id}</span>}
        {proposal.review.reviewer && <span>reviewer {proposal.review.reviewer}</span>}
        <span>{fmtTime(proposal.created_at)}</span>
      </div>

      {proposal.review.reason && (
        <p className="empty-sm">评审意见：{proposal.review.reason}</p>
      )}

      <details className="knowledge-justification">
        <summary>提案变更（{proposal.proposal.changes.length}）</summary>
        <pre>{fmtJson(proposal.proposal.changes)}</pre>
      </details>

      <details className="knowledge-justification">
        <summary>影响面</summary>
        <ul className="knowledge-list">
          <li>目标一致：{String(proposal.review.impact.targetConsistent)}</li>
          <li>新增不可满足类：{proposal.review.impact.newlyUnsatisfiable.length}</li>
          <li>消除不可满足类：{proposal.review.impact.resolvedUnsatisfiable.length}</li>
          <li>新增子类结论：{proposal.review.impact.addedSubsumptions.length}</li>
          <li>移除子类结论：{proposal.review.impact.removedSubsumptions.length}</li>
        </ul>
      </details>

      {stage === 'pending' && (
        <div className="knowledge-actions">
          <input
            value={reason}
            onChange={(event) => setReason(event.target.value)}
            placeholder="批准或拒绝理由"
          />
          <button disabled={busy} onClick={() => void onReview(proposal.id, true, reason)}>
            批准
          </button>
          <button className="ghost" disabled={busy} onClick={() => void onReview(proposal.id, false, reason)}>
            拒绝
          </button>
        </div>
      )}

      {stage === 'approved' && (
        <div className="knowledge-actions">
          <input
            value={targetId}
            onChange={(event) => setTargetId(event.target.value)}
            placeholder="新版本 ID"
          />
          <button disabled={busy || !targetId.trim()} onClick={() => void onApply(proposal.id, targetId)}>
            应用
          </button>
        </div>
      )}
    </article>
  )
}

export function KnowledgePanel() {
  const [current, setCurrent] = useState<KnowledgeVersionDto | null>(null)
  const [diagnosis, setDiagnosis] = useState<KnowledgeDiagnosisDto | null>(null)
  const [versions, setVersions] = useState<KnowledgeVersionDto[]>([])
  const [proposals, setProposals] = useState<OntologyEvolutionRecordDto[]>([])
  const [rollbackTarget, setRollbackTarget] = useState('')
  const [rollbackId, setRollbackId] = useState('')
  const [draftSource, setDraftSource] = useState('')
  const [draftProposalId, setDraftProposalId] = useState('')
  const [draftChanges, setDraftChanges] = useState('')
  const [draftRuntimeIr, setDraftRuntimeIr] = useState('')
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  const [actionErr, setActionErr] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)

  const load = async () => {
    setLoading(true)
    setErr(null)
    try {
      const version = await getKnowledgeCurrent()
      setCurrent(version)
      // Diagnosis is intentionally independent: an inconsistent snapshot must
      // still be visible even when a reasoning extension returns an error.
      setDiagnosis(await diagnoseKnowledge().catch(() => null))
      const governance = await loadGovernance()
      setVersions(governance.versions)
      setProposals(governance.proposals)
    } catch (e) {
      const message = (e as Error).message || ''
      if (message.includes('尚未初始化')) {
        setCurrent(null)
        setDiagnosis(null)
        setVersions([])
        setProposals([])
      } else {
        setErr(message)
      }
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    void load()
  }, [])

  const runAction = async (action: () => Promise<string | void>) => {
    setBusy(true)
    setActionErr(null)
    setNotice(null)
    try {
      const message = await action()
      await load()
      if (message) setNotice(message)
    } catch (e) {
      setActionErr((e as Error).message || '治理操作失败')
    } finally {
      setBusy(false)
    }
  }

  const reviewProposal = async (id: string, approve: boolean, reason?: string) => {
    await runAction(async () => {
      await reviewKnowledgeProposal(id, approve, reason?.trim() ? reason : undefined)
      return approve ? `提案 ${id} 已批准` : `提案 ${id} 已拒绝`
    })
  }

  const applyProposal = async (id: string, targetVersionId: string) => {
    await runAction(async () => {
      const result = await applyKnowledgeProposal(id, targetVersionId)
      return `已创建并切换到本体版本 ${result.version.id}`
    })
  }

  const createProposal = async () => {
    await runAction(async () => {
      const draft = parseKnowledgeProposalDraft({
        proposalId: draftProposalId,
        sourceVersionId: draftSource,
        changesText: draftChanges,
        runtimeIrText: draftRuntimeIr,
      })
      await createKnowledgeProposal(draft.sourceVersionId, {
        id: draft.proposalId,
        changes: draft.changes,
      }, draft.runtimeIr)
      setDraftProposalId('')
      setDraftChanges('')
      setDraftRuntimeIr('')
      return `提案 ${draft.proposalId} 已创建，等待人工评审`
    })
  }

  const requestRollback = async () => {
    const proposalId = rollbackId.trim()
    const targetVersionId = rollbackTarget
    await runAction(async () => {
      await proposeKnowledgeRollback(proposalId, targetVersionId)
      setRollbackId('')
      setRollbackTarget('')
      return `回滚提案 ${proposalId} 已创建，等待人工评审`
    })
  }

  const health = knowledgeHealthLabel(diagnosis)
  const healthKey = diagnosis ? (diagnosis.consistent ? 'healthy' : 'bad') : 'unknown'
  const nextVersionId = knowledgeNextVersionId(versions)

  return (
    <div className="knowledge-panel">
      <div className="evo-head">
        <h3>
          <BookOpen size={16} /> Knowledge · 本体推理与演化
        </h3>
        <button onClick={() => void load()} disabled={loading}>
          <RefreshCw size={13} /> {loading ? '加载中…' : '刷新'}
        </button>
      </div>
      <p className="evo-note">
        查看当前不可变本体版本，对持久化 TBox 与 live World ABox 做推理诊断；评审、应用和回滚均写入持久审计。
        需要 Operator 及以上角色，写操作需 Developer。
      </p>

      {err && <div className="err-msg">⚠️ {err}</div>}
      {!err && actionErr && <div className="err-msg">⚠️ {actionErr}</div>}
      {!actionErr && notice && <div className="success-msg">✓ {notice}</div>}

      {!current && !loading && !err && (
        <div className="empty">知识本体尚未初始化。可通过 API 提交根版本后回到此页查看。</div>
      )}

      {current && (
        <>
          <section className="dash-section">
            <h4>
              <BookOpen size={14} /> 当前版本
            </h4>
            <div className="knowledge-meta">
              <span>
                版本 <code>{current.id}</code>
              </span>
              <span>revision {current.revision}</span>
              <span>axioms {current.ontology.axioms.length}</span>
              <span>{current.parent_id ? `parent ${current.parent_id}` : 'root'}</span>
              <span>{fmtTime(current.created_at)}</span>
            </div>
          </section>

          <section className="dash-section">
            <h4>推理健康</h4>
            <div className="knowledge-status">
              <b className={`knowledge-badge ${healthKey}`}>{health}</b>
            </div>

            {diagnosis && diagnosis.consistent && diagnosis.unsatisfiableClasses.length > 0 && (
              <>
                <h5>不可满足类</h5>
                <ul className="knowledge-list">
                  {diagnosis.unsatisfiableClasses.map((class_) => (
                    <li key={class_}>
                      <code>{class_}</code>
                    </li>
                  ))}
                </ul>
              </>
            )}

            {diagnosis?.unsatisfiable.map(({ class: class_, justification }) => (
              <details key={class_} className="knowledge-justification">
                <summary>{class_} justification</summary>
                <pre>{fmtJson(justification.axioms)}</pre>
              </details>
            ))}

            {diagnosis?.inconsistency && (
              <details open className="knowledge-justification bad">
                <summary>不一致解释</summary>
                <pre>{fmtJson(diagnosis.inconsistency.axioms)}</pre>
              </details>
            )}
          </section>

          <section className="dash-section">
            <h4>
              <GitBranch size={14} /> 受控回滚
            </h4>
            <div className="knowledge-actions">
              <select
                value={rollbackTarget}
                onChange={(event) => setRollbackTarget(event.target.value)}
                disabled={busy || versions.length < 2}
              >
                <option value="">选择历史版本</option>
                {versions
                  .filter((version) => version.id !== current.id)
                  .map((version) => (
                    <option key={version.id} value={version.id}>
                      {version.id} · revision {version.revision}
                    </option>
                  ))}
              </select>
              <input
                value={rollbackId}
                onChange={(event) => setRollbackId(event.target.value)}
                placeholder="回滚提案 ID"
              />
              <button disabled={busy || !rollbackTarget || !rollbackId.trim()} onClick={() => void requestRollback()}>
                创建回滚提案
              </button>
            </div>
            <p className="empty-sm">回滚不是直接覆盖：它会生成 Replace 提案，必须再次具名评审后才能应用。</p>
          </section>

          <section className="dash-section">
            <h4>创建演化提案</h4>
            <div className="knowledge-actions">
              <select
                value={draftSource}
                onChange={(event) => setDraftSource(event.target.value)}
                disabled={busy || versions.length === 0}
              >
                <option value="">选择源版本</option>
                {versions.map((version) => (
                  <option key={version.id} value={version.id}>
                    {version.id} · revision {version.revision}
                  </option>
                ))}
              </select>
              <input
                value={draftProposalId}
                onChange={(event) => setDraftProposalId(event.target.value)}
                placeholder="唯一提案 ID"
              />
            </div>
            <textarea
              className="knowledge-json-editor"
              value={draftChanges}
              onChange={(event) => setDraftChanges(event.target.value)}
              placeholder={`[
  { "Add": { "SubClassOf": { "sub": { "Class": "Puppy" }, "sup": { "Class": "Dog" } } } }
]`}
              rows={6}
              spellCheck={false}
            />
            <textarea
              className="knowledge-json-editor"
              value={draftRuntimeIr}
              onChange={(event) => setDraftRuntimeIr(event.target.value)}
              placeholder="可选 Runtime IR JSON；留空表示继承源版本的运行时投影"
              rows={4}
              spellCheck={false}
            />
            <div className="knowledge-actions">
              <button
                disabled={
                  busy ||
                  !draftSource ||
                  !draftProposalId.trim() ||
                  !draftChanges.trim()
                }
                onClick={() => void createProposal()}
              >
                创建提案
              </button>
              <span className="empty-sm">提交后仍需具名评审，不能直接修改当前版本。</span>
            </div>
          </section>

          <section className="dash-section">
            <h4>演化提案</h4>
            {proposals.length === 0 ? (
              <div className="empty-sm">暂无持久化提案。</div>
            ) : (
              <div className="knowledge-proposals">
                {proposals.map((proposal) => (
                  <KnowledgeProposalCard
                    key={`${proposal.id}-${proposal.created_at}`}
                    proposal={proposal}
                    busy={busy}
                    defaultTargetId={nextVersionId}
                    onReview={reviewProposal}
                    onApply={applyProposal}
                  />
                ))}
              </div>
            )}
          </section>

          <section className="dash-section">
            <h4>版本谱系</h4>
            <ul className="knowledge-list">
              {versions.map((version) => (
                <li key={version.id}>
                  <code>{version.id}</code>
                  <span className="knowledge-version-meta">
                    revision {version.revision} · {version.ontology.axioms.length} axioms ·{' '}
                    {fmtTime(version.created_at)}
                  </span>
                </li>
              ))}
            </ul>
          </section>

          <section className="dash-section">
            <h4>运行时投影</h4>
            {current.runtime_ir ? (
              <details className="knowledge-justification">
                <summary>Runtime IR JSON</summary>
                <pre>{fmtJson(current.runtime_ir)}</pre>
              </details>
            ) : (
              <div className="empty-sm">当前版本没有绑定 Runtime IR。</div>
            )}
          </section>
        </>
      )}
    </div>
  )
}
