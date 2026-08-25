# Business Loop v41

## 目标

持续寻找更优路径不能只依赖人工逐次点击；但自主循环必须有硬边界和安全停止条件。

## 行为

- 新增 `POST /api/v1/business/optimize`，取代一次性批量执行。
- 每轮仍完整执行 `Discover → Decide → Act → Verify`，并复用既有审批、actor、预算、
  资源、世界一致性和租约闸门。
- `maxCycles` 默认 5，最大 20；每个 cycle 都重新发现、规划和读取最新 Outcome 学习证据。
- 响应包含目标指标、初始/剩余差距、已执行动作、每轮闭环明细、学习洞察和
  `stoppedBecause`（GoalReached / NoSafeCandidate / BlockedByApproval / NoActiveGoal /
  MaxCycles）。
- 任一轮没有安全净收益候选，或发现需要人工/审批的中断恢复队列，立即停止。
- UI 提供“执行至多 5 步”，显示实际完成的安全动作数量。

## 验证

两个可自动执行机会在目标还差 1 时会先执行一个；目标仍差 1 时，下一轮基于最新
Outcome 重规划并执行另一个。目标达成后返回 `GoalReached`。最终每个机会只有一份
`ActionExecuted` 证据；审批-only 场景单独验证 `BlockedByApproval`。
