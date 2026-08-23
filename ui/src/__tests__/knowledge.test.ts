import { describe, expect, it } from 'vitest'
import { knowledgeHealth, knowledgeHealthLabel } from '../knowledge'
import type { KnowledgeDiagnosisDto } from '../types'

function diagnosis(overrides: Partial<KnowledgeDiagnosisDto>): KnowledgeDiagnosisDto {
  return {
    consistent: true,
    unsatisfiableClasses: [],
    unsatisfiable: [],
    inconsistency: null,
    ...overrides,
  }
}

describe('knowledgeHealth', () => {
  it('healthy means consistent with no unsatisfiable classes', () => {
    expect(knowledgeHealth(diagnosis({}))).toBe('healthy')
    expect(knowledgeHealthLabel(diagnosis({}))).toBe('一致且可满足')
  })

  it('global inconsistency outranks class-level findings', () => {
    const value = diagnosis({
      consistent: false,
      unsatisfiableClasses: ['Dog'],
      inconsistency: { axiom_indices: [0], axioms: [] },
    })
    expect(knowledgeHealth(value)).toBe('inconsistent')
    expect(knowledgeHealthLabel(value)).toBe('知识库不一致')
  })

  it('consistent ontologies can still contain empty concepts', () => {
    const value = diagnosis({ unsatisfiableClasses: ['Empty'] })
    expect(knowledgeHealth(value)).toBe('unsatisfiable')
    expect(knowledgeHealthLabel(value)).toBe('存在不可满足类')
  })

  it('missing diagnosis remains unknown instead of claiming success', () => {
    expect(knowledgeHealth(null)).toBe('unknown')
    expect(knowledgeHealthLabel(null)).toBe('尚未诊断')
  })
})
import {
  knowledgeNextVersionId,
  knowledgeProposalStage,
  parseKnowledgeProposalDraft,
} from '../knowledge'
import type { OntologyEvolutionRecordDto } from '../types'

function proposal(decision: 'Pending' | 'Approved' | 'Rejected', appliedVersionId?: string): OntologyEvolutionRecordDto {
  return {
    id: 'add-puppy',
    source_version_id: 'root',
    proposal: { id: 'add-puppy', changes: [] },
    review: {
      proposal_id: 'add-puppy',
      impact: {
        currentConsistent: true,
        targetConsistent: true,
        newlyUnsatisfiable: [],
        resolvedUnsatisfiable: [],
        addedSubsumptions: [['Puppy', 'Dog']],
        removedSubsumptions: [],
      },
      decision,
      reviewer: decision === 'Pending' ? null : 'alice',
      reason: null,
    },
    applied_version_id: appliedVersionId,
    created_at: '2026-08-23T00:00:00Z',
  }
}

describe('knowledge governance helpers', () => {
  it('derives proposal stage from review and application state', () => {
    expect(knowledgeProposalStage(proposal('Pending'))).toBe('pending')
    expect(knowledgeProposalStage(proposal('Approved'))).toBe('approved')
    expect(knowledgeProposalStage(proposal('Rejected'))).toBe('rejected')
    expect(knowledgeProposalStage(proposal('Approved', 'v2'))).toBe('applied')
  })

  it('parses a generic proposal envelope and preserves optional runtime IR', () => {
    const parsed = parseKnowledgeProposalDraft({
      proposalId: ' add-puppy ',
      sourceVersionId: ' root ',
      changesText: '[{ "Add": { "SubClassOf": {} } }]',
      runtimeIrText: '{ "object_types": [] }',
    })
    expect(parsed).toEqual({
      proposalId: 'add-puppy',
      sourceVersionId: 'root',
      changes: [{ Add: { SubClassOf: {} } }],
      runtimeIr: { object_types: [] },
    })
    expect(
      parseKnowledgeProposalDraft({
        proposalId: 'pure-dl',
        sourceVersionId: 'root',
        changesText: '[{ "Add": {} }]',
        runtimeIrText: ' ',
      }).runtimeIr,
    ).toBeUndefined()
  })

  it('rejects malformed proposal envelopes before governance submission', () => {
    const draft = {
      proposalId: 'bad',
      sourceVersionId: 'root',
      changesText: 'not-json',
      runtimeIrText: '',
    }
    expect(() => parseKnowledgeProposalDraft(draft)).toThrow('变更 JSON 必须是合法 JSON')
    expect(() =>
      parseKnowledgeProposalDraft({ ...draft, changesText: '[]' }),
    ).toThrow('变更必须是至少一项的 JSON 数组')
    expect(() =>
      parseKnowledgeProposalDraft({ ...draft, changesText: '[{}]' }),
    ).toThrow('每项变更必须有且仅有 Add、Remove 或 Replace 操作')
    expect(() =>
      parseKnowledgeProposalDraft({ ...draft, proposalId: ' ', changesText: '[{ "Add": {} }]' }),
    ).toThrow('提案 ID 不能为空')
  })

  it('suggests the next linear version and avoids collisions', () => {
    const version = (id: string, revision: number) => ({
      revision,
      id,
      parent_id: null,
      ontology: { axioms: [] },
      runtime_ir: null,
      created_at: '2026-08-23T00:00:00Z',
    })
    expect(knowledgeNextVersionId([])).toBe('v1')
    expect(knowledgeNextVersionId([version('root', 1), version('v2', 2)])).toBe('v3')
    expect(knowledgeNextVersionId([version('v3', 2)])).toBe('v3-2')
  })
})
