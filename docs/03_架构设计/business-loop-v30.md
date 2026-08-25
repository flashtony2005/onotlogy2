# Business Loop v30

## 目标

Deferred 动作不再把终局固定记作“成功一次”，而是支持按终局对象上的实际收益量化
目标进展。

## 契约

- Deferred 创建效应可声明：
  `result_benefit_field` 和 `result_benefit_factor`。
- 字段必须是终局对象上的 int/float 属性；两者必须成对声明，系数限定 `[0,1]`。
- 结算成功时读取终局对象的声明字段，写入 `BusinessResult.goal_delta = 值 × 系数`。
- 目标刷新直接信任可回链 Decision/Opportunity 的 `BusinessResult.goal_delta`；
  不再从 ActionExecuted 反查估算值。
- 未声明收益字段的 Deferred 终局保持默认 1。

## 验证

解析测试覆盖成对字段、数值属性和 Immediate 拒绝。E2E 测试在跟进对象上记录
`recovered_amount=500`、系数 0.5，断言目标前进 250。
