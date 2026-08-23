// Knowledge 状态推导保持纯函数，便于在 UI 渲染前测试诊断与治理语义。
import type {
  KnowledgeDiagnosisDto,
  KnowledgeVersionDto,
  OntologyEvolutionRecordDto,
} from './types'

export type KnowledgeHealth = 'unknown' | 'healthy' | 'inconsistent' | 'unsatisfiable'

export type KnowledgeHealthLabel =
  | '尚未诊断'
  | '一致且可满足'
  | '知识库不一致'
  | '存在不可满足类'

export type KnowledgeProposalStage = 'pending' | 'approved' | 'rejected' | 'applied'

export function knowledgeHealth(
  diagnosis: KnowledgeDiagnosisDto | null,
): KnowledgeHealth {
  if (!diagnosis) return 'unknown'
  if (!diagnosis.consistent) return 'inconsistent'
  return diagnosis.unsatisfiableClasses.length > 0 ? 'unsatisfiable' : 'healthy'
}

export function knowledgeHealthLabel(
  diagnosis: KnowledgeDiagnosisDto | null,
): KnowledgeHealthLabel {
  switch (knowledgeHealth(diagnosis)) {
    case 'healthy':
      return '一致且可满足'
    case 'inconsistent':
      return '知识库不一致'
    case 'unsatisfiable':
      return '存在不可满足类'
    default:
      return '尚未诊断'
  }
}

export function knowledgeProposalStage(
  proposal: OntologyEvolutionRecordDto,
): KnowledgeProposalStage {
  if (proposal.applied_version_id) return 'applied'
  switch (proposal.review.decision) {
    case 'Approved':
      return 'approved'
    case 'Rejected':
      return 'rejected'
    default:
      return 'pending'
  }
}

export function knowledgeNextVersionId(versions: KnowledgeVersionDto[]): string {
  const nextRevision = versions.reduce((max, version) => Math.max(max, version.revision), 0) + 1
  const used = new Set(versions.map((version) => version.id))
  let candidate = `v${nextRevision}`
  for (let suffix = 2; used.has(candidate); suffix += 1) {
    candidate = `v${nextRevision}-${suffix}`
  }
  return candidate
}

export interface KnowledgeProposalDraft {
  proposalId: string
  sourceVersionId: string
  changesText: string
  runtimeIrText: string
}

export interface ParsedKnowledgeProposalDraft {
  proposalId: string
  sourceVersionId: string
  changes: Array<Record<string, unknown>>
  runtimeIr?: Record<string, unknown>
}

const CHANGE_OPERATIONS = new Set(['Add', 'Remove', 'Replace'])

/// Parse a generic proposal without trying to duplicate backend DL validation.
///
/// The client only enforces the envelope; axiom semantics stay behind the
/// governance API so an invalid or unsupported change fails closed there.
export function parseKnowledgeProposalDraft(
  draft: KnowledgeProposalDraft,
): ParsedKnowledgeProposalDraft {
  const proposalId = draft.proposalId.trim()
  const sourceVersionId = draft.sourceVersionId.trim()
  if (!proposalId) throw new Error('提案 ID 不能为空')
  if (!sourceVersionId) throw new Error('必须选择源版本')

  let parsedChanges: unknown
  try {
    parsedChanges = JSON.parse(draft.changesText)
  } catch {
    throw new Error('变更 JSON 必须是合法 JSON')
  }
  if (!Array.isArray(parsedChanges) || parsedChanges.length === 0) {
    throw new Error('变更必须是至少一项的 JSON 数组')
  }
  const changes = parsedChanges.map((change) => {
    if (typeof change !== 'object' || change === null || Array.isArray(change)) {
      throw new Error('每项变更必须是 JSON object')
    }
    const operation = Object.keys(change)
    if (operation.length !== 1 || !CHANGE_OPERATIONS.has(operation[0])) {
      throw new Error('每项变更必须有且仅有 Add、Remove 或 Replace 操作')
    }
    return change as Record<string, unknown>
  })

  let runtimeIr: Record<string, unknown> | undefined
  if (draft.runtimeIrText.trim()) {
    let parsedRuntimeIr: unknown
    try {
      parsedRuntimeIr = JSON.parse(draft.runtimeIrText)
    } catch {
      throw new Error('Runtime IR 必须是合法 JSON')
    }
    if (typeof parsedRuntimeIr !== 'object' || parsedRuntimeIr === null || Array.isArray(parsedRuntimeIr)) {
      throw new Error('Runtime IR 必须是 JSON object')
    }
    runtimeIr = parsedRuntimeIr as Record<string, unknown>
  }

  return { proposalId, sourceVersionId, changes, runtimeIr }
}
