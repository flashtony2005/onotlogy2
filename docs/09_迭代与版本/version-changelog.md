# 版本变更日志
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-09-002 |
| 所属模块 | OpenAIP-OS-Version |
| 版本 | V1.1 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
按版本倒序记录新增/修复/优化，作为上线核对依据。

## 2. 业务/技术背景
被 deploy-steps.md 与 rollback-plan.md 引用。

## 3. 核心规则 & 约束（重点）
- [强制] 按版本倒序，固定三级分类：新增功能 / Bug 修复 / 优化调整。
- [强制] 版本号规则：Vx.y.z（主版本。功能迭代。Bug 修复）。

## 4. 详细设计/内容
> 产品版本采用 V0.1~V0.6 路线（见 sprint-plan.md），以下按产品里程碑记录，固定三级分类：新增功能 / Bug 修复 / 优化调整。

## V0.9.0（ToolBackend 缝 + 流水线闸门，2026-08-14）
### 新增
1. `ToolBackend` / `ActionHook`：出世界执行与 pre/post 闸门；默认 `NullBackend` 不影响 CRM
2. Planner `with_backend` / `with_hook`；执行序 pre → backend → ActionEngine → post
3. `examples/code.odl`：File.lint 创建 Finding（参数由后端回填）

## V0.8.7（目标绑定作用对象，2026-08-14）
### 新增
1. `resolve_target_ids`：目标文本点名优先，其次解析 refs，否则不限制
2. `select_candidates` / 重规划按范围收窄；派生任务（`{id}_*` / `customer`）跟随源对象
3. 金额扫描剔除已匹配对象 id；T1–T4 覆盖点名退款/挽留与不点名兼容

## V0.8.6（目标参数抽取，2026-08-14）
### 新增
1. Planner 从 goal/Intent 抽取动作参数，只填 ODL `params:` 声明字段
2. 具名 `amount=80` 优先，否则 Int/Float 取文本最后一个数字；`refund this customer 500` 会改 balance

## V0.8.5（观察层任务视图，2026-08-14）
### 新增
1. `GET /api/v1/wb/agent/tasks` 列出 `RetentionTask`（id/title/customer/status/priority，Pending 优先）
2. 观察 UI 增加 Tasks 标签，Action/Planning 事件实时刷新
3. `oaip object list` 对带 `status` 的对象渲染 `id title status`

## V0.8.4（Intent 过滤动作，2026-08-14）
### 新增
1. Planner 用用户目标（优先）+ 解析 Intent 限制动作族：任务/挽留 → retain/start/complete；退款 → refund；关户 → closeAccount
2. 认不出意图时不过滤，只靠前置条件（兼容旧闭环）

## V0.8.3（一轮目标推到任务稳态，2026-08-14）
### 新增
1. Planner 执行后重快照、重规划（最多 8 波），本轮已执行的 `type:action:target` 不再重复
2. 一条 `agent run` 即可走完 Pending → InProgress → Completed（不引入独立 Workflow）

## V0.8.2（Planner 按状态推进任务，2026-08-14）
### 优化
1. `build_plan` 只纳入当前快照满足前置条件的动作（Pending→start，InProgress→complete；缺参 refund 不误触发）
2. `self.field` 解析兼容世界快照的 `data` 嵌套，状态机前置条件可对真实对象字段生效

## V0.8.1（本体论任务管理，2026-08-13）
### 新增
1. `RetentionTask` 升为一等本体：属性 + `start` / `complete` 状态机（Pending → InProgress → Completed）
2. `Customer.retain` 创建任务时写入 `status=Pending`，供 Planner 在下一轮按本体动作管理任务

## V0.8.0（GraphRAG 基础，2026-08-13）
### 新增
1. `get_object_graph` 方法（storage.rs）——返回对象-关系图
2. `/api/v1/wb/graph` 路由（server.rs）——暴露对象-关系图
3. V0.8 正式起步（GraphRAG 基础，无需新表）

## V0.7.0（用户与账号管理，已实现）
### 新增
1. V0.7 完整用户/账号管理：`oaip user list` / `create` / `create-admin` / `delete`（复用 auth.rs + storage.rs）
2. 账号列表、创建（默认 Developer）、删除（级联会话）
3. 更新 CLI 帮助与 `user` 子命令

## V0.6.1（工程化清理，2026-08-13）
### 优化
1. 移除 Planner legacy 硬编码挽留计划；统一经 ODL `ActionRegistry` 声明式执行（默认 `examples/crm.odl`）
2. 文档品牌统一为 OpenAIP-OS（去除 WorkBuddy 模块前缀）
3. Mutex poison 恢复、审批路径避免 panic；补 README、GitHub Actions CI、Makefile 工具链 PATH
4. 测试全部改为 `Planner::with_actions` + retain 注册表；`cargo test --all-targets` 全绿

## V0.6.0（Evolution AutoApprove，已实现）
### 新增
1. REQ-018：5 道闸门 + 默认关闭 + `wb_approval_audit` 可审计 + 可撤销 AutoApprove
2. CLI/server `--auto-approve-evolution` 与 `POST .../auto-approve` API

## V0.5.0（Agent 自治，已实现）
### 新增
1. Memory 沉淀闭环（REQ-016）与观察 UI Memory 视图
2. Evolution Engine MVP（REQ-017，Learn≠Permission）

## V0.4.0（Action & Policy，已实现）
### 新增
1. 声明式 Action Engine（REQ-014）+ Policy 前置条件/审批闸门（REQ-015）
2. Console / Server 挂起式 Human Approval + HTTP 裁决端点

## V0.3.0（MVP，已实现）
### 新增
1. Agent 闭环：Pi 作 Cognitive Engine + OpenAIP Planner
2. 三层观察 UI + WebSocket 实时轨迹流
3. `TraceEvent(kind=cognitive)` + Trace API
### 优化
1. 统一 `TraceEvent` 模型（world / cognitive 同源）

## V0.2.0（已实现）
### 新增
1. 关系边 `wb_relation_edge`，支撑图查询
2. 领域事件扩展 world 事件语义

## V0.1.0（已实现）
### 新增
1. 极简 Ontology Kernel：`.odl → Compiler → IR → Runtime → Turso → Object CRUD`
2. 统一对象存储 + Event Bus + CLI 观察

## 5. 风险点 & 注意事项
- changelog 与实际发布不符会使回滚失准。

## 6. 关联文档
- [../08_部署运维/deploy-steps.md](../08_部署运维/deploy-steps.md)
- [../08_部署运维/rollback-plan.md](../08_部署运维/rollback-plan.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成 |
| V1.1 | lushaohui | 2026-08-11 | 重写 changelog 为 OpenAIP-OS V0.1~V0.5 产品路线 |
| V1.2 | lushaohui | 2026-08-13 | 同步 V0.1~V0.6 已实现状态；记录 V0.6.1 工程化清理 |

