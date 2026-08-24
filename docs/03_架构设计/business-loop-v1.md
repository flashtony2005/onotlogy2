# Business Loop v1

## 目标

Business Loop v1 把平台的使用方式从“数据 → 查询 → 报表”推进到可执行的运营闭环：

```text
Enterprise State → Opportunity → Decision → Action → Outcome → State Transition → Re-planning
```

该层是上层业务模块，不进入 Kernel。业务概念仍然是普通世界对象，所有写入都通过
`WorldCommand`，因此与既有对象共享项目作用域、事件溯源和审计链。

## 当前范围

第一个闭环落在 `crm-retention` 业务包：

- `BusinessGoal` 描述目标指标、当前值和目标值，HTTP 返回差距视图。
- 引擎扫描项目内 `Customer` 状态；VIP 且超过 45 天未联系时生成 `Opportunity`。
- Opportunity 使用确定性 ID，重复发现不会创建第二条 Open 记录。
- `Decision` 显式记录机会、动作、理由、决策人和状态。
- 执行 `ContactCustomer` 会把客户 `days_since_last_contact` 更新为 0。
- `Outcome` 保存执行前后快照和成功标记；`StateTransition` 保存主体前后状态。
- Opportunity 与 Decision 推进到 `Executed` 后，再规划基于新状态执行。

动作注册表集中在 `src/business_loop.rs::ACTIONS`。新增动作时扩展该表并补充 ODL
能力定义，不在 HTTP 层散落业务分支。

## API

| Method | Path | 说明 |
| --- | --- | --- |
| GET | `/api/v1/business/goals?projectId=...` | 读取 BusinessGoal 差距 |
| POST | `/api/v1/business/discover-opportunities` | `{ "projectId": "..." }` |
| POST | `/api/v1/business/opportunities/:id/decide` | `{ "action": "...", "reason": "..." }` |
| POST | `/api/v1/business/opportunities/:id/execute` | 执行已决策动作 |

读取目标要求 Operator 及以上；发现、决策和执行复用运行触发权限，也要求 Operator
及以上。决策人始终取认证用户，不接受客户端伪造。

## 验证

`tests/business_loop_http.rs` 从真实 `crm-retention` 包安装开始，端到端验证：

1. 目标差距可见；
2. VIP 状态产生唯一 Opportunity；
3. 重复发现保持幂等；
4. Decision/Action/Outcome/StateTransition 全部落库；
5. 客户状态变化后重新发现不再产生同一机会。

## 边界

v1 是最小闭环基线，只覆盖一个可执行规则和一个确定性动作。目标反馈、动作推荐和
单步自动再规划由 [Business Loop v2](./business-loop-v2.md) 继续扩展。
