# Business Loop v47

## 目标

路径学习如果只藏在下一次推荐里，经营者无法审计“更优路径”的历史证据；机会关闭后，
完整动作序列的经验也会从界面消失。

## 行为

- 新增 `GET /api/v1/business/path-insights?projectId=...`。
- 每条证据输出主体类型、完整动作序列、运行数、成功数、实际目标收益总和与成功均值。
- 证据沿用 `(subject_type, actions)` 隔离，只统计能回链 `Outcome ← Decision → Opportunity` 的终局。
- Business Panel 新增路径学习证据区；历史路径不依赖当前 Open Opportunity 继续可见。

## 验证

合同闭环先执行 `DraftTerms → RenewContract`，断言路径证据 API 返回样本 1、成功 1；
CRM HTTP 主流程在 Deferred 挽留终局后断言 `Customer · RetainCustomer` 路径可读取。
UI 通过类型检查、测试和生产构建验证字段接线。
