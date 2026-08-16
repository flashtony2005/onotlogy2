// 纯函数工具：时间/JSON 格式化 + debounce（App 与各面板共用）。
export function fmtTime(ts: string): string {
  const d = new Date(ts)
  if (isNaN(d.getTime())) return ts
  return d.toLocaleTimeString('zh-CN', { hour12: false })
}

export function fmtJson(v: unknown): string {
  if (v == null) return ''
  if (typeof v === 'string') return v
  try {
    return JSON.stringify(v, null, 2)
  } catch {
    return String(v)
  }
}

/// 简单 debounce：窗口内多次调用只执行最后一次（高频 WS 事件触发 REST 刷新合并）。
export function debounce<A extends unknown[]>(fn: (...args: A) => void, ms: number) {
  let t: ReturnType<typeof setTimeout> | null = null
  return (...args: A) => {
    if (t) clearTimeout(t)
    t = setTimeout(() => fn(...args), ms)
  }
}

/// fabric.* 通信事件的专用文案（D：时间线/事件流语义化；未知类型回退原名）。
export function fabricEventLabel(eventType: string): string {
  switch (eventType) {
    case 'fabric.agent_connected':
      return '🔗 Agent 已连接'
    case 'fabric.agent_disconnected':
      return '🔌 Agent 已断开'
    case 'fabric.message_sent':
      return '📤 消息已发出'
    case 'fabric.task_requested':
      return '🎯 任务已请求'
    case 'fabric.task_accepted':
      return '🤝 任务已受理'
    case 'fabric.task_completed':
      return '✅ 任务完成'
    case 'fabric.task_failed':
      return '❌ 任务失败'
    case 'fabric.capability_invoked':
      return '⚡ 能力已调用'
    case 'fabric.approval_requested':
      return '🛂 审批已请求'
    default:
      return eventType
  }
}
