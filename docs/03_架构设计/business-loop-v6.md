# Business Loop v6

## 目标

v5 把 Action Space 迁到 ODL 后，v6 继续移除 Business Loop 里最后一块核心业务
策略硬编码：机会发现规则。`Customer` 现在可以声明 Opportunity Pattern；修改 ODL
就能改变“当前状态里什么值得注意”，而不是修改 Rust。

## ODL 契约

```odl
object Customer:
  properties:
    level: string
    days_since_last_contact: int
  opportunities:
    Reactivation:
      estimated_value: 180000
      evidence: "VIP 客户已 {days_since_last_contact} 天未联系"
      when:
        - field: self.level
          op: eq
          value: "VIP"
        - field: self.days_since_last_contact
          op: gt
          value: 45
```

规则语义：

- `name` 是 `Opportunity.opportunity_type`，也参与幂等去重。
- `when` 条件之间是 AND；字段必须写成 `self.<property>`，值必须是字面量。
- 比较支持 `lt/le/gt/ge/eq/ne`；数值按数量值比较，字符串和布尔只做同型比较。
- `estimated_value` 是非负有限数值，作为单步净价值公式的收益输入。
- `evidence` 是证据模板；`{property}` 会替换成触发对象的字段值。
- 规则必须完整声明 `estimated_value`、非空 `evidence` 和至少一个条件；
  引用未知字段、重复规则名或半声明规则都会在 ODL 解析阶段失败。

## 运行时行为

1. 发现器每次重新解析项目 ODL，然后扫描项目内 `Customer` 状态。
2. 一条规则对一个客户最多保留一个 Open Opportunity；状态变化导致条件不再满足
   时不会自动撤销旧机会，后续决策仍会拒绝非 Open 状态。
3. 创建的 Opportunity、`targets` 关系和后续 Decision / Outcome / StateTransition
   继续走 `WorldCommand`，因此保持事件溯源与审计链。
4. 推荐器继续使用 v5 的 Action Planning 元数据：`benefit * confidence - cost`。

## 验证

- ODL 契约测试覆盖完整规则、半声明规则、空条件和未知字段。
- Business Loop 契约测试读取真实 `crm-retention` 包，验证：
  - 规则创建的机会携带 ODL 声明的价值和渲染后的证据；
  - 把 45 天阈值改成 1000 天后不再发现机会；
  - 动作目录仍由 Action Planning 元数据驱动。

## 边界

v6 只声明“发现什么机会”。目标指标反馈仍绑定 `monthly_reactivations`，
Planner 仍是单步贪心，Actor 还不是第一类本体对象。多机会组合、多步路径搜索、
外部执行通道和人工/Agent 协作分配是下一层能力。
