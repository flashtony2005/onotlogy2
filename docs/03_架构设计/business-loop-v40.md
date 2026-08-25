# Business Loop v40

## 目标

中断执行不能永久滞留在 `Executed` 状态；人工对账后必须有明确的恢复动作。

## 行为

- 新增 `POST /api/v1/business/opportunities/:id/reconcile`。
- 请求必须携带 `acknowledgedNoSideEffect=true`，由人工确认外部副作用不存在。
- 请求还必须绑定具体 `leaseId`；只有 `Expired` 租约可消费。
- 通过校验后，机会从 `Executed` CAS 回到 `Open`，租约标记为 `Reopened`。
- 未确认或副作用可能存在时，系统保持安全空闲，不自动重试。

## 验证

E2E 模拟崩溃后先触发过期扫描；未确认时重开被拒绝。
确认无副作用后，机会变为 `Open`，租约变为 `Reopened`，恢复队列为空，
且不会凭空产生 Outcome 证据。
