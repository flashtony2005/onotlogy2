# Business Loop v46

## 目标

完整动作序列不能脱离本体上下文解释。`Touch → Finish` 在合同、供应商、客户等主体类型上
代表不同业务流程；跨类型合并会把一个本体的经验错误地当成另一个本体的证据。

## 行为

- 路径证据 key 从“动作序列”升级为 `(subject_type, actions)`。
- `Opportunity → subject_rule → ObjectType` 决定主体类型，同一主体的多阶段机会仍可拼接。
- 重放桶同步使用 `(subject_type, subject)`，避免同名 ID 跨对象类型串桶。
- 规划读取路径证据时使用当前机会的 owner `ObjectType`。
- API 字段不变：`outcomeRuns` / `outcomeSuccesses` 表示当前主体类型下的路径样本。

## 验证

单元测试构造两个主体类型的同名 `Touch → Finish` 成功序列，断言各自保留 1 个样本，
不合并成 2 个样本。合同生命周期 E2E 继续验证 `DraftTerms → RenewContract` 的跨机会
同主体路径学习。
