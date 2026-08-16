import { ShieldCheck } from 'lucide-react'

/** 治理视图：策略检查与审批审计（M1d：V1 审计表随门面删除；
 * 审计事实在 Kernel 事件轨，V2 查询入口归 M2 重建）。 */
export function GovernancePanel() {
  return (
    <div className="evolution">
      <div className="evo-head">
        <h3>
          <ShieldCheck size={16} /> 治理 · 策略检查与审批审计
        </h3>
      </div>
      <p className="evo-note">
        Learn≠Permission：知识不自动获得权限。审批审计事实在 Kernel 事件轨
        （approval.requested/granted/rejected/expired/gate_approved），
        V2 查询入口随 M2 落地。
      </p>
      <div className="empty">审批审计查询（V2 事实轨）将于 M2 重建。</div>
    </div>
  )
}
