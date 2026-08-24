# Business Loop v18

## 目标

v17 的命名资源仍只属于单个 Opportunity；v18 补上项目级共享资源池。两个机会消耗同
一 `labor` 池时，第二笔决策和自动执行必须看到第一笔留下的不可变消耗账本。

## ODL 契约

```odl
opportunities:
  Reactivation:
    shared_resource_budget: labor=6
```

- `shared_resource_budget` 可按维度重复声明，格式与 `resource` 相同。
- 同名共享维度在不同 Opportunity 中必须声明相同上限；冲突在 ODL 编译期拒绝。
- 共享池作用于整个项目；动作是否声明 `resource` 决定其消耗量。
- 单机会 `resource_budget` 与项目级共享池同时生效，取更严格的剩余额度。

## 运行时行为

1. 推荐和路径规划用 `ActionExecuted Outcome` 加当前本体重建每个动作的资源消耗。
2. 剩余共享额度进入候选过滤和多步路径预算。
3. 显式决策和执行重复校验机会资源与共享资源，耗尽后 fail-closed。
4. Outcome 引用未知业务动作时拒绝，避免本体演化后旧账本被静默丢弃。

## 验证

ODL 契约覆盖解析、同名上限一致性和冲突拒绝。Business Loop E2E 覆盖两个机会：
第一笔执行后自动推荐为空，第二次显式决策被共享剩余额度拒绝。
