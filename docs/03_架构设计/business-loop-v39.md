# Business Loop v39

## 目标

实例在状态抢占后、副作用完成前崩溃时，系统必须能识别中断执行，避免盲目重复外部副作用。

## 行为

- 新增 `wb_business_execution_lease` 执行租约，同一 Opportunity 只允许一条活跃租约。
- 机会 `Decided → Executed` CAS 成功后立即抢占租约；抢不到租约的机会回滚到 `Decided`。
- 执行成功把租约推进到 `Completed`；执行失败推进到 `Released` 并允许显式重试。
- 租约固定 TTL 为 10 分钟。超长任务需要后续增加续租能力。
- `run-next` 先把过期活跃租约标记为 `Expired`。因为外部副作用未知，过期机会保持
  `Executed`，不自动重试；人工对账后再决定是否创建新机会。

## 验证

存储层用两个连接并发抢占同一机会，断言只有一个 owner 成功；终态只能写一次，
过期扫描只处理已过期租约。
E2E 模拟崩溃：手工写入过期 Claimed 租约并把机会置为 Executed 且无 Outcome，
`run-next` 必须安全空闲，不得产生新的执行证据。
