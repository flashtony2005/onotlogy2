# Business Loop v3

## 目标

v1 验证了状态到状态迁移的最小闭环；v2 让不可变 Outcome 反馈目标并支持单步
自动再规划。v3 解决一个关键缺口：动作目录不再只有一项，系统能对同一机会比较
多个候选路径，选择期望净价值最高的一条，并把比较证据留在 Decision 里。

## 语义

- 动作目录集中在 `BusinessAction` 注册表。每个动作声明先验置信度、成本和执行
  效果；`ContactCustomer` 更新客户接触天数，`RetainCustomer` 创建挽留任务。
- 候选评估使用 `expected_benefit × confidence − cost`。置信度优先使用该项目中
  同动作 Outcome 的成功率，没有历史证据时使用注册表先验。
- 推荐排序仍考虑目标相关性和紧急度：`score = net_value × goal_alignment ×
  urgency_factor`。这样高净值挽留可以胜过低成本接触，而低余额客户的负净价值
  挽留会被自然排除。
- `Decision.alternatives` 保存本轮候选的机器可读 JSON，包含收益、成本、置信度
  和净价值。因此“为什么选它、放弃了什么”成为可审计对象数据，而不是只存在日志
  或模型输出里。
- Opportunity 幂等键是 `subject + opportunity_type`，与当前状态无关。已执行机会
  不能在主体状态未变化时重建，否则会造成重复事实和 ID 冲突。

## 执行效果

`RetainCustomer` 复用 crm-retention 本体中已有的 `Customer.retain` 语义：

1. 创建 `RetentionTask`；
2. 建立 `Customer --needs_retention--> RetentionTask` 关系；
3. 记录成功 Outcome 和 StateTransition；
4. 推进 Decision 与 Opportunity 状态。

挽留任务不直接计入 `monthly_reactivations`。目标进度仍只由满足条件的
`ContactCustomer` Outcome 重建，避免把代理动作误当成经营结果。

## 验证

`tests/business_loop_http.rs` 覆盖双候选路径：

1. 低余额超期 VIP 推荐 `ContactCustomer`；
2. 高余额超期 VIP 推荐 `RetainCustomer`，且候选表中保留落选的接触动作；
3. `run-next` 先执行净价值更高的挽留路径，创建任务和关系，但不改变目标进度；
4. 剩余低余额机会随后由接触路径闭环并推进目标；
5. 所有状态耗尽后返回空闲。

## 边界

v3 的“更优”仍是单步局部最优，不是全局序列规划。收益模型目前来自业务包元数据
和历史成功率；后续应把效果、成本和前置条件进一步声明进 ODL，由运行时统一解释，
而不是继续扩展 Rust 内的业务分支。
