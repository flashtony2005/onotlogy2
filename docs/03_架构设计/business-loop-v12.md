# Business Loop v12

## 目标

v12 补齐 Action 的输入语义。带 `params` 的能力可以先显式决策、验证输入并执行，
不再因为运行时无法求值参数而整体不可用。

## ODL 契约

```odl
actions:
  CollectPayment:
    params:
      amount: float
    planning:
      cost: 100
      prior_confidence: 0.9
      outcome_timing: immediate
    updates:
      - field: balance
        value: amount
    policy:
      precondition:
        - field: amount
          op: le
          value: 1000
```

- 参数名必须由 `params` 声明，值表达式和前置条件可以引用参数。
- 自动推荐和路径规划仍只选择无参数动作；输入必须在决策时显式提供。

## 运行时行为

1. Decision 记录序列化后的 `params`，作为不可变执行证据。
2. 决策时校验缺失、未知、类型错误输入，并重新求值参数化前置条件。
3. 执行时从 Selected Decision 恢复输入，再次校验当前本体后才求值效应。
4. `require_approval` 继续默认拒绝；审批网关不能被参数输入绕过。

## 验证

E2E 覆盖缺失、类型错误、越界前置条件、未知参数和合法输入执行。合法输入的
Decision 证据会驱动状态更新、Outcome 和 BusinessResult。
