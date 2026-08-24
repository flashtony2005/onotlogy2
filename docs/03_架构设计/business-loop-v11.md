# Business Loop v11

## 目标

v11 把执行主体变成 ODL 声明。自动循环只能执行 Agent 动作；Human 动作保留在推荐
和路径规划里，等待显式决策，不能被 `run-next` 偷偷执行。

## ODL 契约

```odl
actions:
  ApproveRenewal:
    planning:
      actor: human
      cost: 100
      prior_confidence: 0.7
      outcome_timing: immediate
```

- `actor` 可选，只接受 `agent` 或 `human`；缺省是 `agent`。
- 路径包含任意 Human 步骤时输出 `requiresHuman: true`。

## 运行时行为

1. 推荐和候选携带声明主体。
2. 自动运行先过滤 `requiresHuman` 路径；无 Agent 路径时返回空闲。
3. 显式决策加执行仍可选择 Human 动作。

## 验证

- Parser 覆盖缺省、`agent`/`human` 和非法值 fail-closed。
- E2E 覆盖高价值 Human 路径可见但被自动运行跳过，Agent 路径继续执行；
  另一项目显式决策可执行同一 Human 动作。
