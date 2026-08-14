import { useState } from 'react'

export function LoginForm({
  error,
  onLogin,
}: {
  error: string | null
  onLogin: (username: string, password: string) => void
}) {
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [submitting, setSubmitting] = useState(false)

  const submit = async () => {
    if (!username.trim() || !password) return
    setSubmitting(true)
    try {
      await onLogin(username.trim(), password)
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div className="login-card">
      <h3>🔐 登录 · OpenAIP-OS</h3>
      <p className="login-note">本地账号认证（V0.7）。首次部署请先配置 WB_BOOTSTRAP_ADMIN_USER/PWD 或运行 oaip user create-admin。</p>
      <input
        className="login-input"
        value={username}
        onChange={(e) => setUsername(e.target.value)}
        placeholder="用户名"
        autoFocus
      />
      <input
        className="login-input"
        type="password"
        value={password}
        onChange={(e) => setPassword(e.target.value)}
        placeholder="口令"
        onKeyDown={(e) => e.key === 'Enter' && void submit()}
      />
      {error && <div className="error">⚠ {error}</div>}
      <button
        className="run login-btn"
        onClick={() => void submit()}
        disabled={submitting || !username.trim() || !password}
      >
        {submitting ? '登录中…' : '登录'}
      </button>
    </div>
  )
}
