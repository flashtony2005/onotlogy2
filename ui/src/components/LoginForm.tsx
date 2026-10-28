import { useState } from 'react'

interface LoginFormProps {
  error: string | null
  onLogin: (username: string, password?: string) => void
}

/** 现代极简建筑风登录卡片：白底圆角、毛玻璃微质感、角色快捷切换 */
export function LoginForm({ error, onLogin }: LoginFormProps) {
  const [username, setUsername] = useState('developer')
  const [password, setPassword] = useState('admin123')
  const [submitting, setSubmitting] = useState(false)

  const submit = (e?: React.FormEvent) => {
    if (e) e.preventDefault()
    if (!username.trim() || !password || submitting) return
    setSubmitting(true)
    Promise.resolve(onLogin(username.trim(), password)).finally(() => setSubmitting(false))
  }

  const quickLogin = (roleUser: string) => {
    setUsername(roleUser)
    setPassword('admin123')
    setSubmitting(true)
    Promise.resolve(onLogin(roleUser, 'admin123')).finally(() => setSubmitting(false))
  }

  return (
    <div className="auth-card">
      <div className="auth-card-head">
        <div className="auth-card-icon">🌿</div>
        <h2>欢迎进入认知空间</h2>
        <p>OpenAIP-OS · Ontological Agent Runtime</p>
      </div>

      {/* 快捷身份选择 */}
      <div style={{ display: 'flex', gap: '8px', marginBottom: '18px', justifyContent: 'center' }}>
        <button
          type="button"
          onClick={() => quickLogin('developer')}
          style={{
            fontSize: '11px',
            padding: '5px 10px',
            borderRadius: '6px',
            border: '1px solid',
            borderColor: username === 'developer' ? 'var(--accent)' : 'var(--border)',
            background: username === 'developer' ? 'var(--accent)' : 'var(--canvas-subtle)',
            color: username === 'developer' ? '#ffffff' : 'var(--text-secondary)',
            fontWeight: username === 'developer' ? 600 : 500,
            cursor: 'pointer',
            transition: 'all 0.15s ease',
          }}
        >
          开发者 Developer
        </button>
        <button
          type="button"
          onClick={() => quickLogin('operator')}
          style={{
            fontSize: '11px',
            padding: '5px 10px',
            borderRadius: '6px',
            border: '1px solid',
            borderColor: username === 'operator' ? 'var(--accent)' : 'var(--border)',
            background: username === 'operator' ? 'var(--accent)' : 'var(--canvas-subtle)',
            color: username === 'operator' ? '#ffffff' : 'var(--text-secondary)',
            fontWeight: username === 'operator' ? 600 : 500,
            cursor: 'pointer',
            transition: 'all 0.15s ease',
          }}
        >
          运营者 Operator
        </button>
        <button
          type="button"
          onClick={() => quickLogin('executive')}
          style={{
            fontSize: '11px',
            padding: '5px 10px',
            borderRadius: '6px',
            border: '1px solid',
            borderColor: username === 'executive' ? 'var(--accent)' : 'var(--border)',
            background: username === 'executive' ? 'var(--accent)' : 'var(--canvas-subtle)',
            color: username === 'executive' ? '#ffffff' : 'var(--text-secondary)',
            fontWeight: username === 'executive' ? 600 : 500,
            cursor: 'pointer',
            transition: 'all 0.15s ease',
          }}
        >
          决策官 Executive
        </button>
      </div>

      <form onSubmit={submit}>
        <label className="auth-field">
          <span className="auth-field-label">用户名</span>
          <div className="auth-input-wrap">
            <svg className="auth-input-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
              <path d="M19 21v-2a4 4 0 0 0-4-4H9a4 4 0 0 0-4 4v2" />
              <circle cx="12" cy="7" r="4" />
            </svg>
            <input
              className="auth-input"
              type="text"
              placeholder="请输入用户名"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              autoComplete="username"
              autoFocus
            />
          </div>
        </label>

        <label className="auth-field">
          <span className="auth-field-label">密码</span>
          <div className="auth-input-wrap">
            <svg className="auth-input-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
              <rect x="4" y="10" width="16" height="10" rx="2" />
              <path d="M8 10V7a4 4 0 0 1 8 0v3" />
              <circle cx="12" cy="15" r="1.4" fill="currentColor" stroke="none" />
            </svg>
            <input
              className="auth-input"
              type="password"
              placeholder="请输入密码"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              autoComplete="current-password"
            />
          </div>
        </label>

        <div className="auth-row">
          <label className="auth-remember">
            <input type="checkbox" defaultChecked />
            <span>记住会话</span>
          </label>
          <a className="auth-link" href="#" onClick={(e) => e.preventDefault()}>
            重置凭证
          </a>
        </div>

        {error && <div className="auth-error">⚠ {error}</div>}

        <button className="auth-submit" type="submit" disabled={submitting || !username.trim() || !password}>
          {submitting ? '进入中…' : '登 录 空 间'}
        </button>
      </form>

      <div className="auth-foot">
        OpenAIP-OS 认知平台 · 统一三元本体架构
      </div>
    </div>
  )
}
