# Business Loop v8

## 目标

v7 完成目标语义声明化后，v8 给路径规划补上资源边界。动作不再只按净值排序；
超过机会成本预算的单步或组合路径会被排除。

## ODL 契约

```odl
opportunities:
  RenewalDue:
    estimated_value: 12000
    cost_budget: 5000
    urgency_field: days_to_expiry
    urgency_horizon: 30
    link_edge: targets_contract
```

- `cost_budget` 是可选的非负有限数值。
- 未声明时机会不设成本上限。
- 预算属于 Opportunity Rule，随本体版本化，而不是运行时可随意改的隐式配置。

## 运行时行为

1. 推荐器过滤成本超过剩余预算的候选；没有可负担候选时不产生推荐。
2. 路径搜索把预算沿有向状态迁移递减，单步和累计成本都必须在预算内。
3. 显式决策和执行层重复校验预算；旧 Decision 不能绕过新本体约束。
4. 低成本多步路径可以替代超预算的高成本捷径。

## 验证

- ODL 解析覆盖可选预算和负数拒绝。
- Business Loop 覆盖全部候选超预算时的空闲结果、显式决策拒绝，以及预算内
  `DraftTerms → RenewContract` 替代超预算 `QuickClose` 的端到端路径。

## 边界

v8 约束单个机会的成本。跨机会共享预算池、时间/人力等多维资源和全局组合优化
仍是后续能力。
