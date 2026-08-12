// REST 客户端：与 api-agent-trace.md 对齐（{ code, msg, data } 信封，前缀 /api/v1/wb/agent）。
// BASE 留空 → 同源（生产由 Rust 服务托管，开发由 Vite 代理转发）。

import type {
  ApiResponse,
  ApprovalAuditDto,
  EvolutionStateDto,
  MemoryListDto,
  RunStartDto,
  RunTimeline,
} from './types'

async function unwrap<T>(res: Response): Promise<T> {
  const json = (await res.json()) as ApiResponse<T>
  if (json.code !== 0 || !json.data) {
    throw new Error(json.msg || `请求失败 (code=${json.code})`)
  }
  return json.data
}

export async function postRun(agentId: string, goal: string): Promise<RunStartDto> {
  const res = await fetch('/api/v1/wb/agent/run', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ agentId, goal }),
  })
  return unwrap<RunStartDto>(res)
}

export async function getTimeline(runId: string, view: string): Promise<RunTimeline> {
  const res = await fetch(
    `/api/v1/wb/agent/run/${encodeURIComponent(runId)}/timeline?view=${view}`,
  )
  return unwrap<RunTimeline>(res)
}

/// `GET /api/v1/wb/agent/memory`：记忆存储全量（V0.5 Memory 视图数据源）。
export async function getMemory(): Promise<MemoryListDto> {
  const res = await fetch('/api/v1/wb/agent/memory')
  return unwrap<MemoryListDto>(res)
}

/// `GET /api/v1/wb/agent/evolution`：Evolution 聚合状态（BL-003 · REQ-017）。
/// 仅暴露知识，不含任何改写 Policy / 绕过 Human Approval 的能力（Learn≠Permission）。
export async function getEvolution(): Promise<EvolutionStateDto> {
  const res = await fetch('/api/v1/wb/agent/evolution')
  return unwrap<EvolutionStateDto>(res)
}

/// REQ-018：切换某条 LearnedKnowledge 的自动放行资格（可撤销，幂等）。
/// `enabled=true` 恢复、`false` 撤销（下次该 action 回退 Human Approval）。
export async function postAutoApproveToggle(
  id: string,
  enabled: boolean,
): Promise<{ id: string; autoApproveEnabled: boolean }> {
  const res = await fetch(`/api/v1/wb/agent/evolution/learned/${id}/auto-approve`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ enabled }),
  })
  return unwrap<{ id: string; autoApproveEnabled: boolean }>(res)
}

/// REQ-018：列出自动放行审计（wb_approval_audit，可追溯至 LearnedKnowledge + run）。
export async function getApprovalAudit(): Promise<ApprovalAuditDto[]> {
  const res = await fetch('/api/v1/wb/agent/evolution/audit')
  return unwrap<ApprovalAuditDto[]>(res)
}
