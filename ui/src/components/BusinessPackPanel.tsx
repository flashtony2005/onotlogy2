import { useEffect, useMemo, useState } from 'react'
import {
  getDomainPackOperations,
  getDomainPacks,
  getSkills,
  installDomainPack,
  setDomainPackEnabled,
  updateDomainPackRuntime,
} from '../api'
import { AutomationPanel } from './AutomationPanel'
import type {
  DomainPackDto,
  DomainPackOperationsDto,
  DomainPackRuntimeDto,
  DomainPackRuntimeUpdateDto,
  SkillDto,
  StrategyRegistryDto,
} from '../types'

interface BusinessPackPanelProps {
  currentRole: string
  onAuthError: (error: unknown) => boolean
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

function statusLabel(pack: DomainPackDto): string {
  if (!pack.valid) return '校验失败'
  if (!pack.installed) return '可安装'
  return pack.enabled ? '运行中' : '已停用'
}

function toUpdate(runtime: DomainPackRuntimeDto): DomainPackRuntimeUpdateDto {
  const {
    enabled,
    source_path,
    watch_enabled,
    poll_interval_seconds,
    auto_sync,
    auto_analyze,
    auto_backtest,
    auto_review,
    strategy_ids,
  } = runtime
  return {
    enabled,
    source_path,
    watch_enabled,
    poll_interval_seconds,
    auto_sync,
    auto_analyze,
    auto_backtest,
    auto_review,
    strategy_ids,
  }
}

function metric(value: number | null | undefined): string {
  return value === null || value === undefined ? '—' : value.toFixed(2)
}

function StrategyTable({ strategies }: { strategies: StrategyRegistryDto[] }) {
  if (strategies.length === 0) return <div className="muted">尚未安装或未发现策略定义。</div>
  return (
    <div className="strategy-table-wrap">
      <table className="strategy-table">
        <thead>
          <tr>
            <th>策略</th>
            <th>家族 / 模式</th>
            <th>版本</th>
            <th>回测</th>
            <th>命中 / 随机基线</th>
            <th>发布</th>
          </tr>
        </thead>
        <tbody>
          {strategies.map((strategy) => (
            <tr key={strategy.id}>
              <td>
                <strong>{strategy.name || strategy.code}</strong>
                <code>{strategy.code}</code>
              </td>
              <td>{strategy.family} · {strategy.mode}</td>
              <td><code>v{strategy.version}</code></td>
              <td>{strategy.backtest_count} 次</td>
              <td>
                <span>红 {metric(strategy.mean_red_hits)} / {metric(strategy.random_baseline_red_hits)}</span>
                <span>蓝 {metric(strategy.mean_blue_hits)} / {metric(strategy.random_baseline_blue_hits)}</span>
              </td>
              <td><span className={`strategy-badge ${strategy.publishable ? 'publishable' : 'experimental'}`}>
                {strategy.publishable ? '允许发布' : '需继续验证'}
              </span></td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

export function BusinessPackPanel({ currentRole, onAuthError }: BusinessPackPanelProps) {
  const [packs, setPacks] = useState<DomainPackDto[]>([])
  const [skills, setSkills] = useState<SkillDto[]>([])
  const [operations, setOperations] = useState<Record<string, DomainPackOperationsDto>>({})
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const canManage = currentRole.toLowerCase() === 'developer'

  const skillsById = useMemo(
    () => new Map(skills.map((skill) => [skill.id, skill])),
    [skills],
  )

  const load = async () => {
    setLoading(true)
    setError(null)
    try {
      const [packList, skillList] = await Promise.all([getDomainPacks(), getSkills()])
      const installed = packList.filter((pack) => pack.valid && pack.installed)
      const operationEntries = await Promise.all(
        installed.map(async (pack) => [pack.id, await getDomainPackOperations(pack.id)] as const),
      )
      setPacks(packList)
      setSkills(skillList)
      setOperations(Object.fromEntries(operationEntries))
    } catch (e) {
      if (!onAuthError(e)) setError(errorMessage(e))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    void load()
  }, [])

  const install = async (pack: DomainPackDto) => {
    setBusy(pack.id)
    setError(null)
    setNotice(null)
    try {
      const result = await installDomainPack(pack.id)
      setNotice(`✅ ${result.pack.name} 已安装：项目 ${result.project_id}，对象 +${result.objects_created}，关系 +${result.relations_created}`)
      await load()
    } catch (e) {
      if (!onAuthError(e)) setError(errorMessage(e))
    } finally {
      setBusy(null)
    }
  }

  const toggle = async (pack: DomainPackDto) => {
    setBusy(pack.id)
    setError(null)
    try {
      await setDomainPackEnabled(pack.id, !pack.enabled)
      setNotice(`${pack.name} 已${pack.enabled ? '停用' : '启用'}`)
      await load()
    } catch (e) {
      if (!onAuthError(e)) setError(errorMessage(e))
    } finally {
      setBusy(null)
    }
  }

  const saveRuntime = async (packId: string) => {
    const operation = operations[packId]
    if (!operation) return
    setBusy(packId)
    setError(null)
    try {
      const runtime = await updateDomainPackRuntime(packId, toUpdate(operation.runtime))
      setOperations((current) => ({
        ...current,
        [packId]: { ...operation, runtime },
      }))
      setNotice('运行配置已保存；自动同步、分析和回测将在调度器启用后执行。')
    } catch (e) {
      if (!onAuthError(e)) setError(errorMessage(e))
    } finally {
      setBusy(null)
    }
  }

  const patchRuntime = (packId: string, patch: Partial<DomainPackRuntimeDto>) => {
    setOperations((current) => {
      const operation = current[packId]
      if (!operation) return current
      return { ...current, [packId]: { ...operation, runtime: { ...operation.runtime, ...patch } } }
    })
  }

  return (
    <div className="domain-packs">
      <section className="domain-packs-hero">
        <div>
          <span className="eyebrow">DOMAIN PACKS · OPERATIONS</span>
          <h2>业务包运行能力</h2>
          <p>
            业务包负责本体和业务事实，运行配置负责数据源、文件监测和自动任务；插件由 Skill
            能力反向匹配，策略注册表只展示独立回测证据，不混合统计与实验评分。
          </p>
        </div>
        <button className="reload" onClick={() => void load()} disabled={loading}>
          {loading ? '读取中…' : '↻ 刷新业务包'}
        </button>
      </section>

      {notice && <div className="ok-msg">{notice}</div>}
      {error && <div className="err-msg">⚠️ {error}</div>}

      <div className="domain-packs-summary">
        <div className="domain-stat"><strong>{packs.length}</strong><span>业务包</span></div>
        <div className="domain-stat"><strong>{packs.filter((pack) => pack.valid).length}</strong><span>校验通过</span></div>
        <div className="domain-stat"><strong>{packs.filter((pack) => pack.enabled).length}</strong><span>运行中</span></div>
        <div className="domain-stat"><strong>{skills.length}</strong><span>可用 Skill</span></div>
      </div>

      {packs.length === 0 && !loading ? (
        <div className="empty">未发现业务包。请将业务包放入 domain-packs 目录。</div>
      ) : (
        <div className="domain-pack-list">
          {packs.map((pack) => {
            const operation = operations[pack.id]
            const runtime = operation?.runtime
            return (
              <article key={pack.id} className={`domain-pack-card ${pack.valid ? '' : 'invalid'}`}>
                <div className="domain-pack-head">
                  <div>
                    <div className="domain-pack-title">
                      <code>{pack.id}</code>
                      <span className={`domain-status ${pack.status}`}>{statusLabel(pack)}</span>
                    </div>
                    <h3>{pack.name || '无效业务包'}</h3>
                    <span className="domain-version">v{pack.version || '—'}</span>
                  </div>
                  <div className="domain-actions">
                    {pack.valid && pack.installed && canManage && (
                      <button className="run secondary" onClick={() => void toggle(pack)} disabled={busy === pack.id}>
                        {busy === pack.id ? '处理中…' : pack.enabled ? '停用' : '启用'}
                      </button>
                    )}
                    {pack.valid && !pack.installed && canManage && (
                      <button className="run" onClick={() => void install(pack)} disabled={busy === pack.id}>
                        {busy === pack.id ? '安装中…' : '安装业务包'}
                      </button>
                    )}
                  </div>
                </div>

                {!pack.valid ? (
                  <div className="domain-error">{pack.error || '业务包校验失败'}</div>
                ) : (
                  <>
                    <div className="domain-metrics">
                      <span><b>{pack.ontology_types}</b> 概念类型</span>
                      <span><b>{pack.bootstrap_objects}</b> 初始对象</span>
                      <span><b>{pack.bootstrap_relations}</b> 关系</span>
                      {pack.minimum_runtime && <span>最低运行时 <b>{pack.minimum_runtime}</b></span>}
                    </div>

                    <div className="domain-skills">
                      <span className="domain-label">关联 Skill</span>
                      {pack.skill_ids.length === 0 && <span className="muted">未声明</span>}
                      {pack.skill_ids.map((id) => {
                        const skill = skillsById.get(id)
                        return <div key={id} className="domain-skill-chip"><code>{id}</code><span>{skill ? `${skill.status} · ${skill.capabilities.length} 能力` : '未加载'}</span></div>
                      })}
                    </div>

                    <div className="domain-plugin-row">
                      <span className="domain-label">关联 Plugin</span>
                      {pack.plugin_ids.length === 0 ? <span className="muted">未匹配宿主插件</span> : pack.plugin_ids.map((id) => <code key={id}>{id}</code>)}
                      {pack.plugin_capabilities.map((capability) => <span key={capability} className="capability-chip">{capability}</span>)}
                    </div>

                    {pack.installed && runtime && operation && (
                      <>
                        <section className="domain-operation-section">
                          <div className="domain-section-head"><div><span className="domain-label">运行配置</span><p>配置先持久化并审计；watcher、任务队列和业务包插件已接入，可在下方观察真实执行。</p></div><button className="run" onClick={() => void saveRuntime(pack.id)} disabled={!canManage || busy === pack.id}>{busy === pack.id ? '保存中…' : '保存配置'}</button></div>
                          <div className="runtime-grid">
                            <label>CSV 数据源路径<input value={runtime.source_path} onChange={(e) => patchRuntime(pack.id, { source_path: e.target.value })} disabled={!canManage} placeholder="/Users/.../dlt.csv" /></label>
                            <label>轮询间隔（秒）<input type="number" min={1} max={86400} value={runtime.poll_interval_seconds} onChange={(e) => patchRuntime(pack.id, { poll_interval_seconds: Number(e.target.value) || 30 })} disabled={!canManage} /></label>
                          </div>
                          <div className="runtime-switches">
                            {([['watch_enabled', '监测文件变化'], ['auto_sync', '自动导入新期号'], ['auto_analyze', '自动统计分析'], ['auto_backtest', '自动回测'], ['auto_review', '可选 Pi Agent 分析']] as const).map(([key, label]) => <label key={key} className="switch-row"><input type="checkbox" checked={runtime[key]} onChange={(e) => patchRuntime(pack.id, { [key]: e.target.checked })} disabled={!canManage} /><span>{label}</span></label>)}
                          </div>
                          <div className="strategy-select"><span className="domain-label">自动任务允许的策略（独立评分）</span>{operation.strategies.map((strategy) => <label key={strategy.id} className="switch-row"><input type="checkbox" checked={runtime.strategy_ids.includes(strategy.code)} onChange={(e) => patchRuntime(pack.id, { strategy_ids: e.target.checked ? [...runtime.strategy_ids, strategy.code] : runtime.strategy_ids.filter((id) => id !== strategy.code) })} disabled={!canManage} /><span>{strategy.name} <code>{strategy.code}</code></span></label>)}</div>
                        </section>

                        <section className="domain-operation-section"><div className="domain-section-head"><div><span className="domain-label">策略注册表</span><p>版本、参数、训练窗口和回测指标；“需继续验证”不能发布。</p></div></div><StrategyTable strategies={operation.strategies} /></section>
                        <AutomationPanel packId={pack.id} packName={pack.name} currentRole={currentRole} onAuthError={onAuthError} />
                      </>
                    )}
                  </>
                )}
              </article>
            )
          })}
        </div>
      )}
    </div>
  )
}
