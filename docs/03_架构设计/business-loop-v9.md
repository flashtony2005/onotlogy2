# Business Loop v9

## 目标

v8 补上成本边界后，v9 移除最后一处 `RetentionTask` 硬编码。任何 Deferred 动作
创建的业务对象都能声明自己的主体字段和 Decision 证据边，并在同一个通用生命周期
API 中启动、终局和学习。

## ODL 契约

```odl
actions:
  RetainCustomer:
    planning:
      outcome_timing: deferred
      benefit_field: balance
      benefit_factor: 0.35
    creates:
      - type: RetentionPlan
        data:
          customer: self
          status: "Pending"
        link: needs_retention
        subject_field: customer
        decision_edge: produced_plan
```

- Deferred 创建效应必须声明：
  - `subject_field`：保存主体 ID 的对象属性，值表达式必须是 `self`；
  - `decision_edge`：`Decision -> created object` 的已声明关系。
- Immediate 创建效应禁止声明这两项；其业务结果仍在执行时立即产生。
- 关系端点、对象属性和数据校验都在 ODL 编译期 fail-closed。

## 运行时行为

1. 执行器沿声明边建立主体关系和 Decision 证据关系，返回真实创建类型。
2. `/api/v1/business/follow-ups/:id/start` 只允许 Pending → InProgress。
3. `/api/v1/business/follow-ups/:id/result` 只允许 InProgress → Completed/Failed。
4. 终局沿 Decision 证据链生成 `BusinessResult`，随后重建目标进度并影响后续
   action confidence。
5. 旧的挽留任务专用运行时路径和 API 已删除。

## 验证

- ODL 解析覆盖完整元数据、缺失元数据和 immediate 滥用拒绝。
- E2E 覆盖非 `RetentionTask` 类型 `FollowUpPlan` 的创建、启动、终局和目标推进。

## 边界

v9 解决单条 Deferred 证据对象的通用性。跨对象工作流编排、人工/Agent 抢占式
协作和全局组合优化仍是后续能力。
