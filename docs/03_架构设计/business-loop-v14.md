# Business Loop v14

## 目标

让 Human-in-the-loop 机会进入推荐视图，同时继续禁止 Agent 自动执行。

## 运行时契约

1. `ActionCandidate` 暴露 `requiresApproval` 和声明参数；只校验主体级前置条件，
   参数级条件仍留给显式决策 fail-closed 校验。
2. 无参 `require_approval` 动作可进入 `OpportunityPath`，并暴露 `requiresApproval`。
3. `run-next` 只选择无参数、无需审批且 Actor 为 Agent 的候选。
4. 审批执行链不变：显式决策 → 独立审批 → 执行。

## 验证

E2E 覆盖：审批候选可见、参数化审批动作不进入路径、自动运行跳过、显式审批后可执行。
