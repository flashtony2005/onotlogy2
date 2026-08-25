# Business Loop v42

## 目标

Deferred 动作创建的跟进对象不能只靠记住对象 ID 才能继续处理；项目必须能看到一个
可观察、可操作的验证队列。

## 行为

- 新增 `GET /api/v1/business/follow-ups?projectId=...`。
- 队列只返回本体声明的 Deferred 创建对象，且状态必须是 `Pending` 或 `InProgress`。
- 每项沿 `link_edge` 回链业务主体，沿 `decision_edge` 回链 Decision，并显示原始动作。
- Business Panel 新增验证队列：Pending 可启动，InProgress 可记录成功或失败。
- 读取使用评估读权限；启动和终局仍复用既有写权限、状态机和目标进度闸门。

## 验证

领域 E2E 覆盖 `Pending → InProgress → 终局后离队`，并确认主体、动作和类型来自 ODL 声明。
HTTP E2E 覆盖队列读取和成功终局后的队列为空；UI 通过 TypeScript 构建验证接线。
