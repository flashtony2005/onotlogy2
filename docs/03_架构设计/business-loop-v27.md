# Business Loop v27

## 目标

企业共享资源池支持滚动时间窗，避免历史消耗永久锁死当前规划。

## 契约

- `shared_resource_window_days` 必须与 `shared_resource_budget` 同时声明。
- 同一本体中所有共享池使用同一个正整数窗口。
- 窗口内账本依据 Outcome 的 `executed_at: datetime` 统计；缺时间或非法时间 fail-closed。
- 未声明窗口时保持累计池；项目指标预算不受影响。

## 行为

1. ODL 解析期校验窗口一致性和资源声明绑定。
2. 执行证据记录 RFC3339 执行时间。
3. 推荐、决策、执行和 Portfolio 都按 `now - window_days` 过滤企业级 Outcome。

## 验证

单元测试验证 30 天窗外样本不计数、窗内样本计数。E2E 验证两个项目的 30 天
共享池：第一个项目执行后，第二个项目仍会被正确拒绝。
