# Business Loop v17

## 目标

v10 只有单一货币池；v17 把命名资源变成机会预算的一等维度。动作可以声明人力、
时间或风险额度，规划、推荐、显式决策和执行都会在同一资源语义上 fail-closed。

## ODL 契约

```odl
opportunities:
  RenewalDue:
    cost_budget: 5000
    resource_budget: labor=5

actions:
  QuickClose:
    planning:
      cost: 4000
      prior_confidence: 0.5
      outcome_timing: immediate
      resource: labor=6
```

- `resource` 和 `resource_budget` 可按维度重复声明，格式为 `name=非负有限数值`。
- 同一动作或同一机会内重复声明同一维度会解析失败。
- 未声明的动作资源不限制；未声明的机会资源预算表示该维度无上限。
- 货币 `cost` / `cost_budget` / `metric_budget` 继续是共享货币约束。

## 运行时行为

1. 推荐候选输出 `resources` 需求，并过滤超过机会命名预算的动作。
2. 多步路径沿每一步递减命名资源；任一中间步骤不可负担即剪枝。
3. 显式决策和执行重复校验预算，超资源 Decision 不能写入。
4. 本版本仍是单个 Opportunity 的资源边界；跨机会共享资源池和预留/释放留给后续版本。

## 验证

ODL 契约覆盖解析、数值拒绝和重复拒绝。Business Loop E2E 覆盖超资源快捷路径被
过滤、显式决策 fail-closed、低资源两步路径自动执行，以及候选资源明细可见。
