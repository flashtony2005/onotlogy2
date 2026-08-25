# Business Loop v51

## 目标

机会发现必须消费本体推理结果，而不是把存储里的精确对象类型当作业务主体全集。
否则声明在父类上的商业规则会漏掉所有子类实例，Ontology 的继承语义也无法进入经营决策。

## 行为

- 机会发现先构建当前项目的完整 TBox/ABox 快照，并通过 DL Reasoner 检索每个主体的实例集合。
- 声明为 `Customer` 的规则可以命中 `VipCustomer extends Customer` 等子类实例。
- 规则条件仍在通过一致性检查后的主体状态上求值；发现、创建 Opportunity 和建立关系保持幂等。

## 验证

新增端到端回归写入一个 `VipCustomer` 实例，断言父类 `Reactivation` 规则经 Reasoner 推理后发现并持久化 Opportunity。
