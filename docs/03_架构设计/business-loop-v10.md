# Business Loop v10

## 目标

v8 只能限制单个机会；v10 让同一经营指标下的多个机会共享资源预算。这样第二个
机会不能在第一个机会已耗尽预算后继续执行，显式决策也不能绕过限制。

## ODL 契约

```odl
opportunities:
  Reactivation:
    estimated_value: 180000
    goal_metric: monthly_reactivations
    metric_budget: 50000
```

- `metric_budget` 可选，必须是非负有限数值。
- 声明时必须同时声明 `goal_metric`。
- 预算按项目内同一 `goal_metric` 共享。

## 运行时行为

1. 每次动作执行写入 `ActionExecuted Outcome.cost` 和 `metric`，形成不可变消耗账本。
2. 推荐、路径规划、决策和执行用 `metric_budget - 已消耗成本` 得出剩余预算。
3. 单机会 `cost_budget` 与共享 `metric_budget` 中更小者为当前可用上限；路径按累计成本递减。
4. 预算耗尽后不再发现可执行推荐或路径；`run-next` 返回空闲，显式决策 fail-closed。

## 验证

- ODL 解析覆盖可选预算、负数拒绝和缺少 goal_metric 拒绝。
- E2E 覆盖两个机会共享 5000 预算：第一笔执行记录 cost=5000，第二机会推荐、
  路径和自动运行为空，显式决策被拒绝。

## 边界

v10 是同一指标的单一成本池。多维资源、时间窗、预留/释放和跨指标组合优化仍属
后续能力。
