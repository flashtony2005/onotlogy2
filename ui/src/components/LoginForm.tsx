import { useState } from 'react'

interface LoginFormProps {
  error: string | null
  onLogin: (username: string, password: string) => void
}

/** 治愈插画风登录卡片：白底圆角、图标输入框、记住我/忘记密码、蓝色主按钮、注册引导。 */
export function LoginForm({ error, onLogin }: LoginFormProps) {
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [submitting, setSubmitting] = useState(false)

  const submit = (e: React.FormEvent) => {
    e.preventDefault()
    if (!username.trim() || !password || submitting) return
    setSubmitting(true)
    Promise.resolve(onLogin(username.trim(), password)).finally(() => setSubmitting(false))
  }

  return (
    <div className="auth-card">
      <div className="auth-card-head">
        <div className="auth-card-icon">🌿</div>
        <h2>欢迎回来</h2>
        <p>登录 OpenAIP-OS · Agent Runtime</p>
      </div>

      <form onSubmit={submit}>
        <label className="auth-field">
          <span className="auth-field-label">用户名 / 邮箱</span>
          <div className="auth-input-wrap">
            <svg className="auth-input-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
              <path d="M4 6h16a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1Z" />
              <path d="m3 7 9 6 9-6" />
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
            <span>记住我</span>
          </label>
          <a className="auth-link" href="#" onClick={(e) => e.preventDefault()}>
            忘记密码？
          </a>
        </div>

        {error && <div className="auth-error">⚠ {error}</div>}

        <button className="auth-submit" type="submit" disabled={submitting || !username.trim() || !password}>
          {submitting ? '登录中…' : '登 录'}
        </button>
      </form>

      <div className="auth-foot">
        还没有账号？<a className="auth-link" href="#" onClick={(e) => e.preventDefault()}>立即注册</a>
      </div>
    </div>
  )
}
