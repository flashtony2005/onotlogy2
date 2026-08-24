# Business Loop v7

## 目标

v6 把“发现什么机会”交给 ODL，但目标反馈仍然绑定 Rust 常量
`monthly_reactivations`。v7 把经营目标语义也声明化：业务包可以定义自己的目标
指标；运行时只按图谱证据链推进目标，不再假设所有业务都围绕复购。

## ODL 契约

```odl
object Customer:
  opportunities:
    Reactivation:
      estimated_value: 180000
      goal_metric: monthly_reactivations
      evidence: "VIP 客户已 {days_since_last_contact} 天未联系"
      when:
        - field: self.level
          op: eq
          value: "VIP"
  actions:
    ContactCustomer:
      planning:
        cost: 5000
        prior_confidence: 0.7
        outcome_timing: immediate
        goal_metric: monthly_reactivations
```

语义：

- `OpportunityRule.goal_metric` 声明机会服务的经营指标。
- `ActionPlanning.goal_metric` 声明动作成功后可推进的经营指标。
- `goal_metric` 是可选声明；未声明的动作或规则不参与目标进度。
- 空声明在 ODL 解析阶段失败。
- 目标进度要求规则和动作的 `goal_metric` 完全一致；不一致时执行仍可产生审计
  结果，但不计入任何 BusinessGoal。

## 运行时行为

1. `refresh_goal_progress` 从不可变 `Outcome` 重建进度，避免重复累加。
2. 只有满足以下完整证据链的成功 `BusinessResult` 才会计数：
   `Outcome(success=true) <- produced_outcome <- Decision -> Opportunity`
3. `Decision.action` 必须解析到 ODL Action Planning；`Opportunity.opportunity_type`
   必须解析到同名 Opportunity Rule；两者的 `goal_metric` 一致才计入对应指标。
4. `BusinessGoal.current_value = baseline_value + 该指标成功结果数`。
5. Planner 用当前 Open Opportunity 的规则、选中动作和 active BusinessGoal 计算
   目标相关性；推荐理由中的指标名来自目标对象，而不是 Rust 常量。
6. immediate 动作在有剩余差距时获得更高对齐权重；deferred 动作保留次高权重，
   因为其业务结果要等待终局证据。

## 验证

- ODL 契约测试覆盖规则与动作的 `goal_metric` 解析以及空值拒绝。
- Business Loop 契约测试覆盖自定义指标端到端推进。
- 规则与动作指标不一致时，动作可执行并留证，但目标进度不变。
- crm-retention 主流程验证 ContactCustomer 与终局成功的 RetainCustomer 都按
  ODL 声明计入 `monthly_reactivations`。

## 边界

v7 解决“目标是什么、哪些结果算进度”，但 Planner 仍是单步贪心；Actor 还不是
第一类本体对象。多步路径搜索、资源约束、外部执行通道、人工/Agent 协作分配，
以及跨指标组合优化留给后续版本。
