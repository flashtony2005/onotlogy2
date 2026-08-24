# Business Loop v2

## 目标

v1 已验证最小事实闭环：`Enterprise State → Opportunity → Decision → Action →
Outcome → StateTransition`。v2 补上闭环最后两块：

1. **目标反馈**：目标进度不再靠人工改 `current_value`，而是从成功 Outcome
   证据重建，保证状态变化可解释、可审计、可重放。
2. **持续再规划**：系统根据活跃目标差距、客户状态紧急度、估计价值和历史动作
   成功率推荐下一动作，并能通过一个受权限保护的 API 自动完成下一轮决策和执行。

## 语义

- `BusinessGoal.baseline_value` 是周期起点；`current_value` 派生为起点加有效
  Outcome 数量。有效复购 Outcome 必须满足：动作是 `ContactCustomer`、成功为真、
  执行前超过 45 天未联系、执行后归零。
- 推荐器只处理 `Open` Opportunity。评分由目标相关性、紧急度、估计价值和经验
  置信度共同决定，排序使用确定性 tie-breaker。
- `/business/run-next` 每次只执行一步，不批量扫射。若没有可用机会，返回
  `executed=false` 和最新目标视图。

## API

| Method | Path | 说明 |
| --- | --- | --- |
| GET | `/api/v1/business/recommendations?projectId=...` | 读取排过序的下一动作 |
| POST | `/api/v1/business/run-next` | `{ "projectId": "..." }`，执行一轮完整再规划 |

读取推荐要求 Operator 及以上；`run-next` 复用运行触发权限。决策人始终来自认证
身份，不由请求体声明。

## 验证

`tests/business_loop_http.rs` 覆盖两条路径：

1. 人工显式决策后，Outcome 使目标从 12 增至 13；
2. 注入第二个超期 VIP 后，`run-next` 能发现、决策、执行并把目标推进到 14；
3. 状态耗尽后再调用 `run-next` 返回空闲，而不是重复创建机会。

## 边界

v2 的“更优”仍是可解释的局部启发式，不是全局最短路径规划。当前动作目录只有一
项，因此经验学习主要建立评分和证据结构；后续增加动作时不需要改变闭环协议，
只需扩展动作表、ODL 效果和历史证据解释。
