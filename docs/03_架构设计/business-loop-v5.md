# Business Loop v5

## 目标

v4 已经把动作证据和业务终局分开。v5 解决另一个关键问题：动作目录不能藏在
Rust 常量里。业务有哪些可规划动作、成本多少、初始置信度多少、结果何时生效，
都必须由项目当前 ODL 声明。这样修改本体就是修改 Agent 的决策空间。

## ODL 契约

只有挂在 `Customer.actions` 下并且带 `planning` 块的动作会进入 Business Loop：

```odl
object Customer:
  actions:
    ContactCustomer:
      planning:
        cost: 5000
        prior_confidence: 0.7
        outcome_timing: immediate
      updates:
        - field: days_since_last_contact
          value: 0

    RetainCustomer:
      planning:
        cost: 20000
        prior_confidence: 0.6
        outcome_timing: deferred
      creates:
        - type: RetentionTask
          data:
            title: "客户挽留计划"
            customer: self
            status: "Pending"
            priority: "MEDIUM"
          link: needs_retention
```

字段语义：

- `cost`：执行一次动作的净成本，非负有限数值。
- `prior_confidence`：没有终局样本时的先验成功率，范围 `[0,1]`。
- `outcome_timing`：`immediate` 表示动作本身直接产生业务结果；`deferred`
  表示必须等待后续任务终局，不允许把“已创建任务”当作成功。
- 没有 `planning` 的动作仍是普通能力（例如退款、任务状态流转），但不会进入
  商业决策；请求这些动作做规划会 fail-closed。
- 非法数值、非法时机或未知 planning 字段会在解析阶段失败。

## 运行时规则

1. 每次机会发现、推荐、决策和执行都重新读取项目 ODL，不使用独立内存目录。
2. 推荐器用 `expected_benefit * confidence - cost` 计算每个候选动作净值。
3. 目标反馈只给 `outcome_timing == immediate` 且已有 `BusinessResult` 的动作计数。
4. 执行器按 ODL 解释 `updates` 和恰好一个 `creates` 效应；创建对象仍通过
   `WorldCommand` 写入，保留项目归属、事件溯源和审计链。
5. 学习仍然只消费 v4 定义的 `stage == BusinessResult` 结果。

## 验证

- 原 HTTP 端到端测试继续覆盖发现、决策、执行、学习与再规划。
- 新增 ODL 契约测试读取同一个业务包文本：
  - 基础 ODL 选择 `ContactCustomer`，并暴露两个候选的成本、置信度和净值；
  - 把 `ContactCustomer.cost` 改为 500000 后，同一客户状态改选
    `RetainCustomer`，证明“ODL 改动即行为改变”；
  - 无 planning 的 `refund` 不能被选作商业动作。

## 边界

v5 只迁移动作目录和单步规划元数据，不引入多步搜索、外部执行适配器或自动
资源调度。它建立的是声明式 Action Space；后续 Business Planner 应在这个空间
上扩展多步路径评估，而不是重新把业务策略硬编码回 Rust。
