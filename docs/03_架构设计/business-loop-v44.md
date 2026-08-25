# Business Loop v44

## 目标

推荐可以内部使用学习账本，但经营者必须能看到真实证据；否则“更优路径”只是黑盒断言。

## 行为

- 新增 `GET /api/v1/business/learning-insights?projectId=...`。
- 证据仍按 `(OpportunityType, Action)` 隔离，避免不同业务上下文互相污染。
- 每条证据输出运行数、成功数、实际目标收益总和以及成功样本平均收益。
- 只统计能沿 `Outcome ← Decision → Opportunity` 回链的 `BusinessResult`；
  孤立结果和状态里程碑不进入学习证据。
- Business Panel 新增学习证据区，展示推荐背后的样本规模和实际交付。

## 验证

领域 E2E 使用量化终局收益 250 断言运行数、成功数和成功均值。HTTP E2E 覆盖
挽留终局后的学习证据 API，UI 通过 TypeScript 构建验证字段接线。
