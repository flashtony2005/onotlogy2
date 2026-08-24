# Business Loop v4

## 目标

v3 已经能在同一机会上比较 `ContactCustomer` 和 `RetainCustomer`，但把“执行了
挽留动作”直接记成成功 Outcome。这个语义会把创建任务误当成客户被留住，进而污
染置信度学习和机会统计。v4 引入两阶段结果生命周期，把动作证据和经营结果分开。

## 语义

- `Outcome.stage` 只有两个值：`ActionExecuted` 表示动作已经发生，`BusinessResult`
  表示业务终局确实发生。
- `RetentionTask.status` 使用显式生命周期：`Pending -> InProgress -> Completed/Failed`。
  失败不是删除状态，而是一次有效的负样本。
- `ContactCustomer` 的动作与效果同步发生，因此一次执行会同时产生
  `ActionExecuted` 和 `BusinessResult`。
- `RetainCustomer` 的动作只是创建任务，因此执行时只产生 `ActionExecuted`
  和 `Decision --produced_task--> RetentionTask` 关系；不修改 Customer 状态，
  不产生 `BusinessResult`。
- 挽留任务进入终局时才生成 `BusinessResult`，并通过原 `produced_task` 边链接回
  `RetainCustomer` 决策。成功和失败都会成为学习样本。
- 置信度学习和目标计数只消费 `stage == BusinessResult` 的结果。这样 Pending 或
  InProgress 任务既不能抬高成功率，也不会误报 `monthly_reactivations`。

## API

新增两个任务生命周期入口：

```http
POST /api/v1/business/retention-tasks/:id/start
POST /api/v1/business/retention-tasks/:id/result
```

`start` 把 `Pending` 推进到 `InProgress`；`result` 接收 `{"success":true|false}`，
把任务推进到 `Completed` 或 `Failed` 并写入终局 Outcome。

## 验证

HTTP 契约测试覆盖完整闭环：

1. 接触动作返回两类结果并推进目标；
2. 挽留动作只返回执行证据，不推进目标，同时保留任务关系；
3. 任务启动、成功终局后仍不误报复购指标；
4. 第二次挽留在只有一条成功业务结果时使用置信度 `1.0`；
5. 失败终局后第三次挽留置信度降到 `0.5`，证明负样本参与学习。

## 边界

v4 解决的是证据阶段混淆，不扩大规划范围。Business Loop 仍然是单步局部最优。
挽留终局目前由外部调用方上报；后续接入真实工作流或人工确认流时，应复用同一
两阶段模型，而不是重新引入“执行即成功”的捷径。
