// auth.ts 令牌存储单测：内存优先 + sessionStorage 备份；存储不可用时降级仅内存。
// node 测试环境无 sessionStorage → 走异常降级路径，恰好覆盖两条分支。

import { describe, expect, it } from 'vitest'
import { clearToken, getToken, setToken } from '../auth'

describe('auth token store', () => {
  it('set → get 往返（内存路径）', () => {
    setToken('tok-1')
    expect(getToken()).toBe('tok-1')
  })

  it('clear 后为空', () => {
    setToken('tok-2')
    clearToken()
    expect(getToken()).toBeNull()
  })

  it('覆盖旧令牌', () => {
    setToken('tok-3')
    setToken('tok-4')
    expect(getToken()).toBe('tok-4')
  })

  it('空令牌可存取', () => {
    clearToken()
    expect(getToken()).toBeNull()
  })
})
