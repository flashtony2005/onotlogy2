# Business Loop v50

## 目标

推荐候选和完整路径必须解释同一个商业事实。若 Immediate 动作在路径规划里学习实际收益，
但单动作推荐仍使用静态估值，会出现两套排名和不可信理由。

## 行为

- 抽出统一收益契约：
  - Immediate：`planning.benefit_field + benefit_factor`
  - Deferred：创建效应的 `result_benefit_field + result_benefit_factor`
- `learned_expected_benefit` 与 `learned_path_benefit` 都只在该契约成立时用成功样本均值。
- 无契约动作保留静态估算；计数型 `goal_delta=1` 不能替换货币或业务量化收益。
- 全失败且契约成立的候选收益为 0，失败概率继续由置信度表达。

## 验证

单元测试断言同一 Immediate 收益账本在推荐候选和路径规划中产生相同收益，
成功样本为 500，全失败样本为 0；无契约动作不误用计数型收益。业务套件和全量门禁回归通过。
