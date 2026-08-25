# Business Loop v26

## 目标

`shared_resource_budget` 从项目内池升级为企业级资源池，约束同一资源在不同
项目间的总消耗。

## 行为

- ODL 键不变：同名 `shared_resource_budget` 在所有机会中仍必须有相同上限。
- ActionExecuted Outcome 把执行时 `resources` 规划快照记录为 JSON 字符串；
  声明共享池的 Outcome 必须定义 `resources: string`。
- 共享余量读取所有项目的 Outcome 快照求和；指标成本预算仍只统计本项目。
- 显式决策、执行、推荐和 Portfolio 使用同一个企业级账本。

## 验证

E2E 测试创建两个项目并声明同一个 `labor=6` 池：第一个项目执行后，第二个项目的
自动运行不选择动作，显式决策也被拒绝。
