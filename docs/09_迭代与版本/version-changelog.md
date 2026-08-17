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

## V2 架构落地（可执行本体内核 + V2 认知闭环 + 投影体系，2026-08-15 ~ 08-17）
> 架构版本 V2（区别于产品版本号 V0.x）。本周期完成 V1 planner 退役、演化引擎归一，
> 并落地 World Model 投影体系（Phase 5–8）与 Agent Fabric 审批 / 异步 / 增量三阶段。

### 新增
1. **M1 退役 V1 planner**：删 `src/planner/`（747 行）、`action.rs`、`RuntimeKernel` 门面、V1 三表（wb_run/wb_memory/wb_approval）；CLI `agent run` 与 server `/run` 改走 V2 cognitive loop；v0_3_* 测试退役，由 `tests/v2_parity/` 契约测试覆盖（G1–G10 钉绿）
2. **M2 演化引擎归一**：`evolution2`（Success/Failure 双通道 + Gate）= 经验层；`rule/evolution.rs` = 行为规则层；`skill` mutation = 资产层；删旧 `evolution.rs` + 三表 + `/evolution` 端点
3. **Phase 5–8 投影体系（projection-v1.md）**：P1 纯函数（Event Log→Kernel 同态）/ P2 幂等（applied 水位）/ P4 确定性（normalize_ts 微秒→秒轴）/ P5 写入路径同事务 / P6 沟通黑匣子入世界 / P7 裸写不可追溯；`Projection` 模块 + `project_from_storage`
4. **T1 Agent Fabric 审批接入**：Bridge 挂起态 + `fabric_resolve` 复用 `/run/:run_id/approval`；HTTP `facade_fabric_submit` 返回挂起态、pending 走审批流
5. **T2b 异步契约**（已实现，待合入 master：`feat/t2b-realtime`）：`Bridge::submit` 立即返 `running` + `correlation_id`，子进程 `spawn_blocking` 后台执行（fire-and-forget）；CLI 轮询审计事件拿终局；tc_f9 端到端 live cancel
6. **P3 增量投影（watermark）**：世界关系 + watermark 持久化于 `wb_meta`（JSON blob，零 schema 迁移）；`after_id` 只应用新事件，跨 run 不再全量重放；`tc_pj9` 钉增量终局==全量重放

### 已知缺口（已修复，见 tech-debt-reduction-plan.md 与 fix/survivor-gaps）

> 以下三项原于 V1.3 诚实声明为「仍存缺口」，已于 `fix/survivor-gaps` 全部修复并钉测试：

1. ~~存储正确性~~ → 已修复：`link()` 加 `uq_edge_active` 部分唯一索引（活跃边不重复，并发双插归 WB2007）；`update()` 加 `WHERE version=?` 乐观锁 + `StorageError::Conflict`（并发后写覆盖前写归 Conflict，重试最新版本成功）。
2. ~~投影类型丢失~~ → 已修复：投影写 `Relation.metadata` 保留类型化值（数值不文本化），`EntityView` 出边优先取 `metadata`，`tc_o4` 钉 `amount=100 → Number(100)`。
3. ~~Facade 契约方法未暴露~~ → 已修复：`fabric.cancel`/`fabric.stream`/`capability.discover`/`session.get` 落地为 `FabricService` + `Facade` 固有方法（facade-v1.md V1.4）。

## V0.10.0（模块化重构 + 认证收口 + 测试基建，2026-08-14）
### 优化
1. 长文件模块化：planner(1855行)/storage(1662)/server(1371) 拆目录模块共 22 文件；odl 去重；App.tsx 拆 4 组件
2. V0.7 认证收口：恢复 `/auth/me` + 静态 fallback；UI 登录链路（token sessionStorage + Bearer + WS `?token=`）
3. 安全：/graph /trace 角色钳制、登录限流（5 次锁 5 分钟）、会话令牌 SHA-256 哈希、LIKE 通配符转义
4. 正确性：update 原子递增、link 事务化、WS 订阅-缓冲-补历史、审批超时落库、知识按 (类型,动作) 键控
5. 测试：v0_3(3084行) 按领域拆 5 文件；tests/common 统一 FakeEngine/http_call/ODL 常量（4 文件复用）
6. 工程：版本号统一 0.9.0；pre-commit 质量门禁（.githooks，fmt/clippy/test）

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
| V1.3 | lushaohui | 2026-08-18 | 补 V2 架构落地条目（M1/M2 退役 V1 planner + 演化归一、Phase 5–8 投影、T1/T2b/P3），并诚实声明存储/投影/Facade 三项仍存缺口 |
| V1.4 | lushaohui | 2026-08-18 | 修复 V1.3 声明三项缺口（fix/survivor-gaps）：① `link()` 加 `uq_edge_active` 部分唯一索引杜绝并发双插、`update()` 加 `WHERE version` 乐观锁 + `Conflict`；② 投影写 `Relation.metadata` 保留数值类型、`EntityView` 出边优先取 metadata；③ Facade 暴露 `fabric.cancel/stream`/`capability.discover`/`session.get`（facade-v1.md V1.4）；全部钉测试（storage 并发 link/update、tc_o4、facade/session 单测） |

