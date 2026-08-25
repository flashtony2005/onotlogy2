# Business Loop v29

## 目标

目标进度从“业务结果次数”升级为“可度量的主体状态收益”，让再规划对经营目标的
贡献量化。

## 契约

- `Outcome` 新增 `goal_delta: float`。
- Immediate 动作执行时，用声明的 `benefit_field/benefit_factor` 折算主体前后差值，
  写入 `ActionExecuted.goal_delta`；未声明时默认 1。
- Immediate 的 `BusinessResult` 复制同一收益。
- 目标刷新仍只信任成功且可回连 Decision/Opportunity 的 `BusinessResult`，但累加
  同一决策的 `ActionExecuted.goal_delta`。
- 非法或缺失账本收益会 fail-closed；Deferred 终局暂时保持一次计数。

## 验证

单元测试验证 `1000 → 1500 * 0.5 = 250`。E2E 测试验证 ContactCustomer 将客户
`lifetime_value` 从 1000 更新到 1500 后，目标当前值增加 250 而不是 1。
