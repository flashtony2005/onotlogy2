# Business Loop v13

## 目标

v13 让 `require_approval` 成为可完成的 Human-in-the-loop 门禁，而不是永久拒绝。
决策、审批和执行拆成三段可审计事实。

## ODL 契约

```odl
actions:
  Refund:
    params:
      amount: float
    planning:
      cost: 100
      prior_confidence: 0.9
      outcome_timing: immediate
    policy:
      require_approval: true
      precondition:
        - field: amount
          op: le
          value: 1000
```

```odl
relation requires_approval:
  source: Decision
  target: Approval
```

## 运行时行为

1. 显式决策先校验预算、参数和前置条件；需要审批时 Decision 进入
   `PendingApproval`，并创建 Pending Approval。
2. `/api/v1/business/decisions/:id/approval` 只允许独立审批人裁决。
   批准后 Decision 回到 Selected；拒绝后 Opportunity 回到 Open。
3. 执行器必须找到 Decision 指向的 Approved Approval，否则拒绝执行。
4. 自动推荐和路径规划继续排除审批动作。

## 验证

E2E 覆盖：Pending 不能直接执行、自批拒绝、批准后执行成功、拒绝后机会回到 Open。
