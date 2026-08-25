# Business Loop v21

## 目标

把 Business Loop 的外部能力执行接入已有 WorkerRegistry，同时保留插件宿主路径。

## 契约

ODL 动作 planning 现在可选择两种外部宿主之一：

- `plugin: <id>` + `capability: <name>`：继续走 PluginCatalog。
- `worker: <id>` + `capability: <name>`：按 ID 查找 WorkerRegistry。

两者不能同时声明；没有外部宿主时仍执行本地声明式 effects。推荐结果会显示
`worker`、`capability`，执行证据的 `provider` 记录实际宿主 ID。

## 执行行为

1. 执行前检查 Worker 已注册且健康。
2. 调用 Worker 能力并要求 `WorkerResult.ok == true`。
3. Worker 输出先经过 v20 的本体属性校验，再作为声明字段权威投影。
4. 任一步失败都会阻止主体状态更新和 Outcome 写入。

## 运行时集成

HTTP Business Loop 的 `run-next` 和机会 execute 现在共享 AppState 中已引导的
WorkerRegistry；该注册表现在使用异步读写锁，避免跨 await 持有 std 锁导致
Handler 失去 Send 边界。
