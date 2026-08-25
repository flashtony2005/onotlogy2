# Business Loop v19

## 目标

v18 能限制跨机会共享资源，但自动运行仍直接取第一条路径。v19 先把所有机会路径
组合成一个可执行 Portfolio：同主体行动互斥，共享资源和指标预算在选中项之间预留。

## 组合契约

- 输入是全部 Open Opportunity 的正净值路径。
- 每个 Opportunity 只保留一条路径；每个 Subject 同一轮只保留一条路径。
- 按全局分数贪心选择，并逐项扣减 `goal_metric` 货币预算和共享命名资源。
- Human / Approval 路径仍保留在 Portfolio 中；自动运行只从无人工、无审批子集选择。

## 运行时行为

1. `GET /api/v1/business/portfolio` 返回可组合的路径集合。
2. `run-next` 改为从 Portfolio 的 Agent 可执行子集中选择第一项。
3. 执行层继续做预算、策略和本体一致性校验；Portfolio 只是规划预留，不是授权。

## 验证

单元测试覆盖高分同主体路径互斥与共享资源预留；既有 Business Loop E2E 保证闭环
不回退。精确全局最优组合仍是后续能力。

## 边界

当前使用有界贪心组合，不做指数级分支定界；跨项目资源池和时间窗仍未建模。
