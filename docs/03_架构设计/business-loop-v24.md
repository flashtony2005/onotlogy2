# Business Loop v24

## 目标

把 Worker 执行统计用于下一次选择，使系统在同能力候选中持续偏向更可靠执行者。

## 契约

ODL 的 `worker` 新增通配值：

```odl
worker: "*"
capability: touch_customer
```

- `worker: <id>` 仍表示显式指定，不参与评分。
- `worker: "*"` 复用 WorkerRouter，在声明该能力的注册 Worker 中选择。
- Router 输入包含 `wb_worker_stats` 派生的 learned trust；样本不足时不修正。
- Outcome 和 BusinessResult 的 `provider` 写入实际选中的 Worker ID。

## 安全边界

Router 先按能力策略过滤，再评分；Business Loop 只对 ODL 已声明且已通过业务
前置检查的能力放行。选中后仍要求健康检查、`WorkerResult.ok`、本体 schema 校验。

## 验证

单元测试先证明无样本时选择高信任 Worker；写入 5 次失败与 5 次成功后，
低声明信任但高成功率的 Worker 会胜出。
