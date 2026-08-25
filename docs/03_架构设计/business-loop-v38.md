# Business Loop v38

## 目标

同一机会不能被两个服务实例重复执行。共享资源账本解决了预算超扣，但非资源型动作仍需要状态抢占。

## 行为

- 新增世界命令 `CompareSetStatus`，在数据库事务内校验对象当前状态并原子写入新状态。
- `apply_world` 的写事务升级为 `IMMEDIATE`，让状态变更在跨实例场景获得数据库锁仲裁。
- 执行机会在完成全部前置校验后先执行 `Decided → Executed` CAS；抢不到的实例直接失败。
- 抢占后的执行如果失败，会自动把机会回滚到 `Decided`，允许显式重试。
- 成功路径中 Decision 使用 `Selected → Executed` CAS 推进；Opportunity 不再重复普通更新。
- 数据库连接配置 5 秒 busy timeout，短暂锁竞争优先等待而不是立即暴露给业务层。

## 验证

存储层测试用两个独立连接并发 CAS，断言恰好一个成功。
E2E 用两个独立存储连接并发执行同一已决策机会，断言只有一个执行结果，
另一个只能收到状态抢占错误；最终 Opportunity 为 `Executed` 且只产生一次动作证据。
