# Business Loop v45

## 目标

单动作学习还不足以证明“更优路径”。经营者需要知道一条完整动作序列是否真的交付过结果，
并用这些真实终局校准后续规划，而不是继续依赖静态置信度。

## 行为

- 沿 `Outcome ← Decision → Opportunity` 回链，按 subject 重放不可变 Outcome 账本。
- 以全局单调 Trace ID 排序；`ActionExecuted` 累积路径，`BusinessResult` 记录完整序列终局。
- 路径证据按精确动作序列聚合运行数、成功数和实际目标收益；孤立 Outcome 不参与学习。
- 用 Laplace 平滑成功率 `(successes + 1) / (runs + 2)` 校准整条路径置信度，
  避免单次失败把有价值的路径清零。
- `/api/v1/business/opportunity-paths` 返回 `outcomeRuns` / `outcomeSuccesses`，
  reason 输出“路径样本N成功M”；Business Panel 展示路径样本规模和成功数。

## 验证

合同闭环 E2E 先执行 `DraftTerms → RenewContract` 并产生成功终局；第二个合同机会重规划时
断言同一路径携带样本 1、成功 1、置信度下降且理由包含路径证据。UI 通过类型检查、单元测试
和生产构建验证字段接线。
