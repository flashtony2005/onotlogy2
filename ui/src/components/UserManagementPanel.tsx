import { useEffect, useState, type FormEvent } from 'react'
import {
  KeyRound,
  RefreshCw,
  ShieldCheck,
  UserPlus,
  UserRoundCheck,
  UserRoundX,
} from 'lucide-react'
import { createUser, getUsers, resetUserPassword, setUserDisabled, setUserRole } from '../api'
import type { UserDto } from '../types'

const ROLES = [
  { value: 'executive', label: 'Executive · 管理层' },
  { value: 'operator', label: 'Operator · 操作员' },
  { value: 'developer', label: 'Developer · 开发者' },
]

interface UserManagementPanelProps {
  currentUsername: string
  onAuthError: (error: unknown) => boolean
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

export function UserManagementPanel({ currentUsername, onAuthError }: UserManagementPanelProps) {
  const [users, setUsers] = useState<UserDto[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [busy, setBusy] = useState<string | null>(null)

  const [username, setUsername] = useState('')
  const [displayName, setDisplayName] = useState('')
  const [password, setPassword] = useState('')
  const [role, setRole] = useState('operator')

  const [resetFor, setResetFor] = useState<UserDto | null>(null)
  const [resetPassword, setResetPassword] = useState('')

  const load = async () => {
    setLoading(true)
    setError(null)
    try {
      setUsers(await getUsers())
    } catch (e) {
      if (!onAuthError(e)) setError(errorMessage(e))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    void load()
  }, [])

  const runAction = async (key: string, action: () => Promise<void>, success: string) => {
    setBusy(key)
    setError(null)
    setNotice(null)
    try {
      await action()
      setNotice(success)
      await load()
    } catch (e) {
      if (!onAuthError(e)) setError(errorMessage(e))
    } finally {
      setBusy(null)
    }
  }

  const submitCreate = async (event: FormEvent) => {
    event.preventDefault()
    await runAction(
      'create',
      async () => {
        await createUser({ username, password, role, displayName })
        setUsername('')
        setDisplayName('')
        setPassword('')
        setRole('operator')
      },
      '用户已创建',
    )
  }

  const submitReset = async (event: FormEvent) => {
    event.preventDefault()
    if (!resetFor) return
    await runAction(
      `reset:${resetFor.id}`,
      async () => {
        await resetUserPassword(resetFor.id, resetPassword)
        setResetPassword('')
        setResetFor(null)
      },
      `已重设 ${resetFor.username} 的密码，旧会话已失效`,
    )
  }

  const changeRole = (target: UserDto, nextRole: string) => {
    void runAction(
      `role:${target.id}`,
      async () => {
        await setUserRole(target.id, nextRole)
      },
      `已更新 ${target.username} 的角色`,
    )
  }

  const toggleDisabled = (target: UserDto) => {
    void runAction(
      `disabled:${target.id}`,
      async () => {
        await setUserDisabled(target.id, !target.disabled)
      },
      target.disabled ? `已启用 ${target.username}` : `已禁用 ${target.username}，其会话已失效`,
    )
  }

  return (
    <div className="user-admin">
      <div className="dash-head">
        <div>
          <h3><ShieldCheck size={17} /> 用户与角色管理</h3>
          <span className="dash-meta">仅 Developer 可用 · 密码只在提交时传输，页面不显示密码哈希</span>
        </div>
        <button className="reload" onClick={() => void load()} disabled={loading || busy !== null}>
          <RefreshCw size={13} /> {loading ? '加载中…' : '刷新'}
        </button>
      </div>

      {error && <div className="err-msg">⚠️ {error}</div>}
      {notice && <div className="user-notice">✓ {notice}</div>}

      <section className="dash-section user-create-section">
        <h4><UserPlus size={14} /> 创建用户</h4>
        <form className="user-create-form" onSubmit={(e) => void submitCreate(e)}>
          <label>用户名<input value={username} onChange={(e) => setUsername(e.target.value)} placeholder="如 operator-01" required /></label>
          <label>显示名称<input value={displayName} onChange={(e) => setDisplayName(e.target.value)} placeholder="可选" /></label>
          <label>初始密码<input type="password" value={password} onChange={(e) => setPassword(e.target.value)} placeholder="至少 8 个字符" minLength={8} required /></label>
          <label>角色<select value={role} onChange={(e) => setRole(e.target.value)}>{ROLES.map((item) => <option key={item.value} value={item.value}>{item.label}</option>)}</select></label>
          <button className="user-primary" type="submit" disabled={busy !== null}>
            <UserPlus size={14} /> {busy === 'create' ? '创建中…' : '创建用户'}
          </button>
        </form>
      </section>

      <section className="dash-section">
        <h4><ShieldCheck size={14} /> 用户列表 <span className="user-count">{users.length}</span></h4>
        {loading && users.length === 0 ? (
          <div className="empty-sm">正在读取用户列表…</div>
        ) : users.length === 0 ? (
          <div className="empty-sm">暂无用户。</div>
        ) : (
          <div className="user-table-wrap">
            <table className="user-table">
              <thead><tr><th>用户</th><th>角色</th><th>状态</th><th>操作</th></tr></thead>
              <tbody>
                {users.map((target) => {
                  const isSelf = target.username === currentUsername
                  const roleBusy = busy === `role:${target.id}`
                  const disabledBusy = busy === `disabled:${target.id}`
                  const resetBusy = busy === `reset:${target.id}`
                  return (
                    <tr key={target.id}>
                      <td><div className="user-name">{target.username}{isSelf && <span className="user-self">当前账号</span>}</div><div className="user-display-name">{target.displayName || '—'}</div></td>
                      <td>
                        <select className="user-role-select" value={target.role} disabled={isSelf || roleBusy || busy !== null} onChange={(e) => changeRole(target, e.target.value)} aria-label={`${target.username} 角色`}>
                          {ROLES.map((item) => <option key={item.value} value={item.value}>{item.label.split(' · ')[0]}</option>)}
                        </select>
                      </td>
                      <td><span className={`user-status ${target.disabled ? 'disabled' : 'enabled'}`}>{target.disabled ? '已禁用' : '已启用'}</span></td>
                      <td>
                        <div className="user-actions">
                          <button className="user-action" disabled={busy !== null} onClick={() => setResetFor(target)} title="重设密码"><KeyRound size={13} /> 重设密码</button>
                          <button className={`user-action ${target.disabled ? 'enable' : 'disable'}`} disabled={isSelf || busy !== null} onClick={() => toggleDisabled(target)} title={isSelf ? '不能禁用当前账号' : undefined}>
                            {target.disabled ? <UserRoundCheck size={13} /> : <UserRoundX size={13} />} {disabledBusy ? '处理中…' : target.disabled ? '启用' : '禁用'}
                          </button>
                        </div>
                        {resetBusy && <span className="user-row-hint">正在重设…</span>}
                      </td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
          </div>
        )}
      </section>

      {resetFor && (
        <div className="user-modal-backdrop" role="presentation" onClick={() => setResetFor(null)}>
          <form className="user-modal" onSubmit={(e) => void submitReset(e)} onClick={(e) => e.stopPropagation()}>
            <h4><KeyRound size={15} /> 重设密码 · {resetFor.username}</h4>
            <p>提交后，该用户的旧会话会立即失效。</p>
            <label>新密码<input autoFocus type="password" value={resetPassword} onChange={(e) => setResetPassword(e.target.value)} placeholder="至少 8 个字符" minLength={8} required /></label>
            <div className="user-modal-actions"><button type="button" className="user-secondary" onClick={() => setResetFor(null)}>取消</button><button type="submit" className="user-primary" disabled={busy !== null}>{busy === `reset:${resetFor.id}` ? '提交中…' : '确认重设'}</button></div>
          </form>
        </div>
      )}
    </div>
  )
}
