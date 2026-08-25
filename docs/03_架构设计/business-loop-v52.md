# Business Loop v52

## 目标

目标导向优化不能只存在于 HTTP 响应里；否则服务重启或用户离开页面后，
“为什么执行这些动作、目标差距如何变化、为什么停止”的证据链会丢失。

## 行为

- `optimize_toward_goals` 在返回前把完整运行写入
  `wb_business_optimization_run`，包含 id、时间戳、每轮闭环明细、学习洞察和停止原因。
- 新增 `GET /api/v1/business/optimization-runs?projectId=...`，按项目返回最新 50 条历史。
- 运行记录表保留 `goal_metric` 和 `stopped_as` 索引列；payload 是权威的完整运行快照。
- Business Panel 展示优化历史的初始/剩余差距、动作数、循环数和停止原因。

## 验证

- 存储层回归验证历史按项目隔离并按时间倒序返回。
- 端到端优化后，通过 API 读回同一条运行记录。
- 目标达成场景验证持久化 payload 的 `stoppedBecause=GoalReached`。
