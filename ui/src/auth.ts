// 认证令牌存储：内存优先 + sessionStorage 备份。
// 不用 localStorage：令牌越久驻留越扩大 XSS/窃取面；sessionStorage 随标签页关闭即失效。

const KEY = 'openaip.token'

let memoryToken: string | null = null

export function setToken(token: string): void {
  memoryToken = token
  try {
    sessionStorage.setItem(KEY, token)
  } catch {
    /* 隐私模式等场景静默降级为仅内存 */
  }
}

export function getToken(): string | null {
  if (memoryToken) return memoryToken
  try {
    const t = sessionStorage.getItem(KEY)
    if (t) memoryToken = t
    return t
  } catch {
    return null
  }
}

export function clearToken(): void {
  memoryToken = null
  try {
    sessionStorage.removeItem(KEY)
  } catch {
    /* ignore */
  }
}
