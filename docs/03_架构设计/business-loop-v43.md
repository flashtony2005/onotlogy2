# Business Loop v43

## 目标

Deferred 动作必须拥有可结算的终局载体；否则机会会停在 `Executed`，既没有
`BusinessResult`，也无法重新进入规划。

## 行为

- ODL 解析期强制 `outcome_timing: deferred` 的动作创建且只创建一个跟进对象。
- 该对象必须声明主体回链和 Decision 证据边；可选收益字段仍保持成对契约。
- 没有终局载体的 Deferred 动作在编译期 fail-closed，不允许进入运行时死路。
- 无目标指标的 Immediate 动作是状态里程碑：记录执行和状态变化，但不借用机会估计价值，
  也不伪造 `BusinessResult` 或污染动作学习账本。
- 里程碑成功仍由 `ActionExecuted.success=true` 证明；只有绑定经营目标的终局才进入
  `BusinessResult`、目标进度和动作学习。

## 验证

解析测试覆盖 Deferred 缺少创建效应时拒绝。通用合同闭环把中间“起草条款”
改为 Immediate 无目标指标，继续验证两步路径、预算选择和状态变化。
