// WebSocket 实时轨迹流：订阅 /api/v1/wb/agent/{id}/stream。
// 断线后指数退避重连，并以 ?after={lastEventId} 补齐历史（与 api-agent-trace.md 4.6 一致）。
// {id} 为 runId 或 agentId；连接建立先推全量历史，再增量推送。
// V0.7 认证：浏览器 WebSocket 无法设置 Authorization 头，令牌经 ?token= 查询参数传递（服务端仅 WS 路径接受）。

import { useEffect, useRef } from 'react'
import { getToken } from '../auth'
import type { TraceEventDto } from '../types'

/// 重连退避上限：指数退避最多 10s；超过 MAX_RETRIES 次（≈ 数分钟）后停止，提示用户刷新。
const MAX_RETRIES = 8
const MAX_BACKOFF_MS = 10000

interface Options {
  /** 登录态门：false 时不连接（避免未登录时发起 WS），登录后自动重连。 */
  enabled?: boolean
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
    if (!id || opts.enabled === false) {
      onStatusRef.current?.(false)
      return
    }
    lastIdRef.current = null // 新 run：从头拉全量历史
    let ws: WebSocket | null = null
    let closedByUs = false
    let retry = 0
    let timer: ReturnType<typeof setTimeout> | null = null

    const connect = () => {
      // U1：令牌已清（登出/401）→ 立即停止重连；重试达上限 → 停止并提示（避免无限刷无效令牌）
      if (!getToken() || retry >= MAX_RETRIES) {
        onStatusRef.current?.(false)
        return
      }
      const params = new URLSearchParams()
      const after = lastIdRef.current
      if (after) params.set('after', after)
      const token = getToken()
      if (token) params.set('token', token)
      const qs = params.toString()
      const url = `${location.origin}/api/v1/wb/agent/${encodeURIComponent(id)}/stream${qs ? `?${qs}` : ''}`
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
        retry = Math.min(retry + 1, MAX_RETRIES)
        timer = setTimeout(connect, Math.min(1000 * 2 ** retry, MAX_BACKOFF_MS))
      }
      ws.onerror = () => ws?.close()
    }

    connect()
    return () => {
      closedByUs = true
      if (timer) clearTimeout(timer)
      ws?.close()
    }
  }, [id, opts.enabled])
}
