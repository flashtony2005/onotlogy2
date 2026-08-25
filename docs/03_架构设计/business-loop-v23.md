# Business Loop v23

## 目标

让动作成功率学习绑定业务上下文，避免同名动作在不同机会类型间互相污染。

## 行为

- 学习样本仍只取 `stage == BusinessResult` 的 Outcome。
- 每个样本必须沿 `Outcome ← Decision → Opportunity` 回链；孤立证据不参与学习。
- 统计键从全局 `action` 升级为 `(OpportunityType, Action)`。
- 无样本时继续使用动作的 `prior_confidence`。

## 验证

单元测试验证同一 `Touch` 动作在 `WinBack` 学到 0%、在 `Upgrade` 学到 100%，
两个上下文互不影响。Business Loop E2E 保证既有学习与闭环不回退。
