// WebSocket 实时轨迹流：订阅 /api/v1/wb/agent/{id}/stream。
// 断线后指数退避重连，并以 ?after={lastEventId} 补齐历史（与 api-agent-trace.md 4.6 一致）。
// {id} 为 runId 或 agentId；连接建立先推全量历史，再增量推送。

import { useEffect, useRef } from 'react'
import type { TraceEventDto } from '../types'

interface Options {
  onEvent: (e: TraceEventDto) => void
  onStatus?: (connected: boolean) => void
}

export function useTraceStream(id: string | null, opts: Options) {
  const lastIdRef = useRef<string | null>(null)
  const onEventRef = useRef(opts.onEvent)
  const onStatusRef = useRef(opts.onStatus)
  onEventRef.current = opts.onEvent
  onStatusRef.current = opts.onStatus

  useEffect(() => {
    if (!id) return
    lastIdRef.current = null // 新 run：从头拉全量历史
    let ws: WebSocket | null = null
    let closedByUs = false
    let retry = 0
    let timer: ReturnType<typeof setTimeout> | null = null

    const connect = () => {
      const after = lastIdRef.current
        ? `?after=${encodeURIComponent(lastIdRef.current)}`
        : ''
      const url = `${location.origin}/api/v1/wb/agent/${encodeURIComponent(id)}/stream${after}`
      ws = new WebSocket(url)
      ws.onopen = () => {
        onStatusRef.current?.(true)
        retry = 0
      }
      ws.onmessage = (ev) => {
        try {
          const e = JSON.parse(ev.data as string) as TraceEventDto
          lastIdRef.current = e.id
          onEventRef.current(e)
        } catch {
          /* 忽略畸形消息 */
        }
      }
      ws.onclose = () => {
        onStatusRef.current?.(false)
        if (closedByUs) return
        retry = Math.min(retry + 1, 6)
        timer = setTimeout(connect, Math.min(1000 * 2 ** retry, 10000))
      }
      ws.onerror = () => ws?.close()
    }

    connect()
    return () => {
      closedByUs = true
      if (timer) clearTimeout(timer)
      ws?.close()
    }
  }, [id])
}
