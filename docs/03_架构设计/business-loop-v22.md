# Business Loop v22

## 目标

让 Worker 执行进入可学习账本，补齐“执行 → 观测 → 长期选择”的反馈链。

## 行为

Business Loop 调用 Worker 后会把 `capability`、`worker_id`、成功状态和耗时写入
既有 `wb_worker_stats`：

- 正常返回但 `ok == false` 记录为失败，并阻止主体状态与 Outcome 写入。
- Worker 抛错记录为失败，同样 fail-closed。
- 成功输出继续按 v20 校验并投影到本体声明字段。

## 验证

E2E 覆盖成功样本 `runs=1, success=1`；失败样本 `runs=1, success=0`，并验证
失败不改变客户状态、不产生业务证据。后续路由器可用这些统计比较同能力 Worker。
