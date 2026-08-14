// format.ts 纯函数单测：时间格式化、JSON 展示、debounce。

import { describe, expect, it, vi } from 'vitest'
import { debounce, fmtJson, fmtTime } from '../format'

describe('fmtTime', () => {
  it('合法时间戳 → 本地时间', () => {
    const ts = new Date(2026, 7, 14, 9, 30, 0).toISOString()
    expect(fmtTime(ts)).not.toBe(ts)
  })

  it('非法输入 → 原样返回', () => {
    expect(fmtTime('not-a-date')).toBe('not-a-date')
    expect(fmtTime('')).toBe('')
  })
})

describe('fmtJson', () => {
  it('null/undefined → 空串', () => {
    expect(fmtJson(null)).toBe('')
    expect(fmtJson(undefined)).toBe('')
  })

  it('字符串原样返回', () => {
    expect(fmtJson('plain')).toBe('plain')
  })

  it('对象 → 缩进 JSON', () => {
    expect(fmtJson({ a: 1 })).toBe('{\n  "a": 1\n}')
  })

  it('循环引用 → 兜底 String()', () => {
    const o: Record<string, unknown> = {}
    o.self = o
    expect(fmtJson(o)).toBe(String(o))
  })
})

describe('debounce', () => {
  it('窗口内多次调用只执行一次（最后一次）', () => {
    vi.useFakeTimers()
    const fn = vi.fn()
    const d = debounce(fn, 100)
    d('a')
    d('b')
    d('c')
    expect(fn).not.toHaveBeenCalled()
    vi.advanceTimersByTime(99)
    expect(fn).not.toHaveBeenCalled()
    vi.advanceTimersByTime(1)
    expect(fn).toHaveBeenCalledTimes(1)
    expect(fn).toHaveBeenCalledWith('c')
    vi.useRealTimers()
  })

  it('窗口过后再次调用可再次触发', () => {
    vi.useFakeTimers()
    const fn = vi.fn()
    const d = debounce(fn, 50)
    d()
    vi.advanceTimersByTime(60)
    d()
    vi.advanceTimersByTime(60)
    expect(fn).toHaveBeenCalledTimes(2)
    vi.useRealTimers()
  })
})
