# Business Loop v48

## 目标

只学习“是否成功”还不够。两条成功率相同的路径可能交付不同经营收益；
路径规划必须区分声明过的货币/量化收益和普通的完成计数，避免用 `goal_delta=1`
错误替换静态估算值。

## 行为

- 完整路径证据继续聚合终局 `goal_delta`。
- 仅当最后一步存在一致的收益契约时，用成功样本的实际均值替换静态收益：
  - Immediate：`planning.benefit_field + benefit_factor`
  - Deferred：首个创建效应的 `result_benefit_field + result_benefit_factor`
- 无收益契约的路径保留静态机会估值；成功概率仍由 Laplace 平滑单独校准。
- 全部失败且已量化的路径收益为 0；失败率不再和零收益双重惩罚未执行规划。
- 路径 reason 输出 `路径样本N成功M`；有收益契约时追加 `收益均值X`。

## 验证

单元测试覆盖 Immediate 收益契约、Deferred 终局收益契约、无契约计数回退、
全失败清零以及可解释证据文本。业务 HTTP 套件验证现有路径规划和学习语义不回归。
