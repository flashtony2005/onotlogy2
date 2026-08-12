// REST 客户端：与 api-agent-trace.md 对齐（{ code, msg, data } 信封，前缀 /api/v1/wb/agent）。
// BASE 留空 → 同源（生产由 Rust 服务托管，开发由 Vite 代理转发）。

import type { ApiResponse, MemoryListDto, RunStartDto, RunTimeline } from './types'

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
