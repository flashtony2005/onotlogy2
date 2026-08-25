# Business Loop v34

## 目标

失败不能终结学习闭环。Deferred 跟进失败后，系统必须保留同一机会并重新规划。

## 行为

- Deferred 终局失败仍记录 `BusinessResult(success=false, goal_delta=0)`。
- 原机会从 `Executed` 重新打开为 `Open`，并在响应中返回
  `reopenedOpportunity`。
- 机会发现保持幂等，不会为重开机会创建重复对象。
- 下一轮推荐使用包含失败的证据账本；失败动作置信度下降后重新参与比较，
  只有替代动作净值更高时才切换。
- 同分时全新机会优先于重试机会，避免失败重试饿死新需求。
- 同一机会最多因失败重开一次；重试再失败不继续重开。

## 验证

E2E 验证 `RetainCustomer` 全部失败后机会重开、无重复发现，且推荐转向
`ContactCustomer`；回放测试同时验证全新机会优先、仍有正净值的动作可有界重试。
