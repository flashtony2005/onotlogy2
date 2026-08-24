# Business Loop v16

## 目标

Action 不再只能做声明式状态改写；可以调用宿主插件里的外部 Capability，并把结果投影回企业状态。

## ODL 契约

```odl
actions:
  TouchCustomer:
    planning:
      cost: 100
      prior_confidence: 0.9
      outcome_timing: immediate
      plugin: crm-plugin
      capability: touch_customer
    updates:
      - field: days_since_last_contact
        value: 0
```

`plugin` 与 `capability` 必须成对声明。

## 推荐契约

候选明细携带已声明的 `plugin` 和 `capability`；本地动作不输出这两个字段。推荐阶段因此可以解释“下一步要调用什么”，执行阶段再通过 PluginCatalog 消费同一契约。

## 运行时契约

1. 执行前按主体、类型、当前状态和参数构造能力输入。
2. 通过共享 PluginCatalog 显式调用声明的 plugin + capability。
3. 输出只能包含并必须补齐 Action `updates` 声明的字段；未知字段和缺失字段都拒绝。
4. 外部动作的 Outcome 必须声明 `provider` 和 `capability`，执行与业务结果证据都会记录来源。
5. 能力调用失败时机会仍停留在 Decided，不写入局部状态更新或 Outcome。

## 验证

E2E 覆盖插件调用、输入证据、输出投影、provider/capability 证据链；单元测试覆盖未知与缺失输出字段拒绝。
