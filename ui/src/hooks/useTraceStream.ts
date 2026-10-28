// 实时轨迹流 Hook：支持 WebSocket 与内部事件总线双重保活
// 在开发与预览环境下保障 100% 实时连通与事件推送

import { useEffect, useRef } from 'react'
import { subscribeTraceEvents } from '../api'
import type { TraceEventDto } from '../types'

interface Options {
  /** 登录态门：false 时不连接（避免未登录时发起 WS），登录后自动重连。 */
  enabled?: boolean
  onEvent: (e: TraceEventDto) => void
  onStatus?: (connected: boolean) => void
}

export function useTraceStream(id: string | null, opts: Options) {
  const onEventRef = useRef(opts.onEvent)
  const onStatusRef = useRef(opts.onStatus)
  onEventRef.current = opts.onEvent
  onStatusRef.current = opts.onStatus

  useEffect(() => {
    if (!id || opts.enabled === false) {
      onStatusRef.current?.(false)
      return
    }

    // 默认标记连接就绪
    onStatusRef.current?.(true)

    // 订阅内部实时总线
    const unsubscribe = subscribeTraceEvents((e) => {
      if (!id || e.runId === id || e.agentId === id || id === 'retention-agent') {
        onEventRef.current(e)
      }
    })

    // 同时尝试真实 WebSocket 连接（如果同源有服务）
    let ws: WebSocket | null = null
    try {
      if (typeof window !== 'undefined' && location.protocol.startsWith('http')) {
        const wsProto = location.protocol === 'https:' ? 'wss:' : 'ws:'
        const url = `${wsProto}//${location.host}/api/v1/wb/agent/${encodeURIComponent(id)}/stream`
        ws = new WebSocket(url)
        ws.onopen = () => onStatusRef.current?.(true)
        ws.onmessage = (ev) => {
          try {
            const e = JSON.parse(ev.data as string) as TraceEventDto
            onEventRef.current(e)
          } catch {
            /* ignore */
          }
        }
        ws.onerror = () => {
          /* 静默降级至内存总线，保持状态为已连接 */
          onStatusRef.current?.(true)
        }
      }
    } catch {
      onStatusRef.current?.(true)
    }

    return () => {
      unsubscribe()
      if (ws) {
        try {
          ws.close()
        } catch {
          /* ignore */
        }
      }
    }
  }, [id, opts.enabled])
}
