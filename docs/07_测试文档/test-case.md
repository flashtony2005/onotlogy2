# 测试用例（测试契约表）
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-07-002 |
| 所属模块 | WorkBuddy-Test |
| 版本 | V2.7 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
把验收标准落成**可执行测试契约**：一条测试定义一个能力的边界（输入→输出→异常）。测试编号与 REQ 一一对应，是 TDD「Red 先行」的直接产物。

## 2. 业务/技术背景
- 依赖 `02_需求设计/requirement-all.md`（REQ）、`02_需求设计/acceptance-criteria.md`（验收）、`07_测试文档/test-plan.md`（TDD 总纲）。
- 每条 Test 在 Task 动工前先写成失败（Red），实现后转绿。

## 3. 核心规则 & 约束（重点）
- [强制] 用例需覆盖正常、边界、异常三类。
- [强制] 用例与 REQ / 验收标准 / 版本可追溯（`TC-<REQ序号>`）。
- [强制] 每条用例含 Given / When / Then，Then 即边界判定，禁止模糊表述。
- [强制] 涉及事件的可解释性用例，必须断言 `object_refs` 非空（缺失即失败）。

## 4. 详细设计/内容

### 4.1 V0.1 测试契约（Object Kernel + 观察基建）
| 用例ID | REQ | 类型 | Given | When | Then（边界判定） |
|--------|-----|------|-------|------|------------------|
| TC-001 | REQ-001 | 单元 | `customer.odl`（name:string, level:string） | compile | 解析为 IR 含 `ObjectType`；缺 property / 未知类型 → 返回明确错误码 |
| TC-002 | REQ-002 | 单元 | 合法 .odl | compile → IR | IR 的 `ObjectType`/`PropertyDef` 字段名与类型与 .odl 完全一致，无信息丢失 |
| TC-003 | REQ-003 | 单元 | 合法 IR | codegen | 生成 `Customer.rs`(struct+`CUSTOMER_DEF`)/`schema.sql`/`ontology.json`，可编译且与 IR 一致 |
| TC-004 | REQ-004 | 集成 | 空 `wb_ontology_object` | create/get/update/query | CRUD 生效；update 使 version 自增；query 支持按 type 过滤 |
| TC-005 | REQ-005 | 集成 | LibSQLStorage | 读写 Turso | 落库数据可被 query 取回 |
| TC-005-b | REQ-005 | 集成 | 旧 schema 文件（残留 object_type/from_id/to_id/edge_type，无 wb_meta） | `LibSQLStorage::open` | 打开即拒 WB2101，提示 `oaip db reset`；不出现 cryptic 的 no such column |
| TC-006 | REQ-006 | e2e | customer.odl | `oaip compile/run/object list` | `oaip object list Customer` 输出 `001 Apple VIP` / `002 Tesla GOLD` |
| TC-007 | REQ-007 | 集成 | Object CRUD 操作 | create/update/delete | 各产生 `TraceEvent(kind=world)`（ObjectCreated/Updated/Deleted），绑定 `object_refs` 并落 `wb_trace_event`；缺 `object_refs` 被拒 |
| TC-008 | REQ-008 | 集成 | RuntimeEvent | Event Bus 分发 | `wb_trace_event` 写入与发布一致；`oaip event list` 可查看 world 事件 |

### 4.2 V0.2 测试契约（World Model = Relation + 领域 Event）
| 用例ID | REQ | 类型 | Given | When | Then（边界判定） |
|--------|-----|------|-------|------|------------------|
| TC-012 | REQ-012 | 集成 | 空 `wb_relation_edge` | link(001, owns, 002) | 建边成功；`neighbors(001)` 含 002；产生 `TraceEvent(kind=world, RelationLinked)` 绑定 `[001,002]` |
| TC-013 | REQ-013 | 集成 | 一次领域事件 | emit_domain_event(PaymentLate, [001]) | 落库 world 事件 PaymentLate；经 EventBus 发布，订阅者收到同一事件 |

### 4.3 V0.3 测试契约（Agent 闭环 = MVP）
| 用例ID | REQ | 类型 | Given | When | Then（边界判定） | 状态 |
|--------|-----|------|-------|------|------------------|------|
| TC-009 | REQ-009 | e2e | 一次 Agent 运行 | 三视图 + WebSocket | 三视图渲染；`WS /api/v1/wb/agent/{id}/stream` 推送 cognitive 事件；Timeline 展示 Goal→…→MemoryUpdate 链；断线重连补齐 | 通过（自动化 tests/v0_3.rs） |
| TC-010 | REQ-010 | 集成 | mock 目标 "reduce churn" | Pi（AgentEngine）`reason()` | 每步认知产出 `TraceEvent(kind=cognitive)` 且绑定 `object_refs`；Observation UI 可回放认知过程 | 通过（自动化 tests/v0_3.rs） |
| TC-011 | REQ-011 | 集成 | 一个 goal | OpenAIP Planner | 跑通 Goal→LLMGoalParser→Planner→PlanStep→Capability→Execute；PlanStep 经 Trace 回放且绑定 `object_refs` | 通过（自动化 tests/v0_3.rs） |
| TC-014 | REQ-011 | 集成 | 3 个客户（001/002/003） | OpenAIP Planner | 每客户 1 个 `RetentionTask` + 1 条 `needs_retention` 边；闭环随候选数线性缩放 | 通过（自动化 tests/v0_3.rs） |
| TC-015 | REQ-011 | 边界 | 空世界（无客户） | OpenAIP Planner | 全部认知事件兜底绑定 `world` 哨兵（`object_refs` 非空，WB2006 防护）；不创建任务 | 通过（自动化 tests/v0_3.rs） |
| TC-016 | REQ-009 | 集成 | 一次 Agent 运行 | `TraceQuery` 过滤 + `?after` 游标 | `for_run`/`for_agent`/`with_object` 过滤正确；`with_after(first_id)` 返回其后全部（不含游标自身） | 通过（自动化 tests/v0_3.rs） |
| TC-017 | REQ-009 | e2e | 运行完毕 | `GET /api/v1/wb/agent/trace?pageSize=2` / `GET /trace/:id` | 列表 `code=0` 含 total+分页；缺失 `id` 返回 `WB2002(code=2002)` | 通过（自动化 tests/v0_3.rs） |
| TC-018 | REQ-009/010 | e2e | `POST /api/v1/wb/agent/run` | 缺 `agentId` → `WB1001`；有效 → `runId`+`streamUrl`；轮询 `/run/:id/timeline?view=executive` | 门禁正确；Executive 视图返回决策摘要（`decision`） | 通过（自动化 tests/v0_3.rs） |
| TC-019 | REQ-009 | e2e | 两个 run 落同一 kernel | `WS /api/v1/wb/agent/{run_b}/stream` | 仅收 `run_b` 事件，不混入 `run_a`（按 run/agent 隔离） | 通过（自动化 tests/v0_3.rs） |

### 4.3.1 V0.4 测试契约（完整 Action 引擎，REQ-014）
| 用例ID | REQ | 类型 | Given | When | Then（边界判定） | 状态 |
|--------|-----|------|-------|------|------------------|------|
| TC-020a | REQ-014 | 单元 | ODL 含 `actions:` 块（params / updates / creates / data / link / `self` 表达式） | `parse_odl` → IR | `ActionDef` 的 params/updates/creates 字段名与类型与 .odl 一致；`self` / `self.field` / 参数名 / 字面量解析正确，无信息丢失 | 通过（自动化 tests/v0_3.rs::tc014a_odl_parses_actions） |
| TC-020b | REQ-014 | 集成 | ActionRegistry + ActionEngine + 已存在目标对象（含 updateLevel / createOrder 声明） | `invoke`（updateLevel 改写 level / createOrder 建 Order+places 边+customer=self） | level 被改写；新 Order 创建且 `places` 边去重（幂等）；产生 `action.createOrder` world 事件并绑定 `object_refs` | 通过（自动化 tests/v0_3.rs::tc014b_action_engine_executes） |
| TC-020c | REQ-014 | e2e | `examples/crm.odl`（`Customer.actions.retain` → 建 RetentionTask + needs_retention 边）经 registry | Planner 经 `ActionRegistry` 闭环（替代硬编码挽留计划） | 每客户 1 个 RetentionTask + 1 条 `needs_retention` 边；`action.retain` world 事件发出；Policy Allow 放行 | 通过（自动化 tests/v0_3.rs::tc014c_planner_via_registry_closed_loop） |

### 4.3.2 V0.4 测试契约（Policy 引擎 + 交互式 Human Approval，REQ-015）
| 用例ID | REQ | 类型 | Given | When | Then（边界判定） | 状态 |
|--------|-----|------|-------|------|------------------|------|
| TC-015a | REQ-015 | 单元 | ODL 含 `actions.<name>.policy:` 块（`require_approval` + `precondition` 子句） | `parse_odl` → IR | `PolicyDef.require_approval` 解析正确；`precondition` 子句（field/op/value，含 `self.field` 与字面量）全部解析，无信息丢失 | 通过（自动化 tests/v0_3.rs::tc015a_odl_parses_policy） |
| TC-015b | REQ-015 | 单元 | 含 `require_approval` 动作的 `ActionDef` + `PlanStep::Action` | `Policy::check_action`（AutoApprover allow=false/true） | allow=false → Deny（WB2005）；allow=true → Allow | 通过（自动化 tests/v0_3.rs::tc015b_approval_gate） |
| TC-015c | REQ-015 | 单元 | refund 动作（precondition：amount≤1000 且 level≠BLACKLIST）+ 参数/目标值 | `Policy::check_action`（前置条件求值） | amount=500 & 非黑名单 → Allow；amount=2000 → Deny；level=BLACKLIST → Deny | 通过（自动化 tests/v0_3.rs::tc015c_precondition_eval） |
| TC-015d | REQ-015 | e2e | 含 `require_approval` 动作的对象 + Planner 闭环 | `run_goal`（默认拒 / 注入 AutoApprover{allow:true}） | 默认 → WB2005 PolicyDenied（提示需 Human Approval）；自动批准 → Completed 且产出 `action.<name>` world 事件 | 通过（自动化 tests/v0_3.rs::tc015d_planner_policy_gate） |
| TC-021a | REQ-015 | 契约 | `oaip` 二进制 | `agent run --help` / `serve --help` | 两个子命令均暴露 `--auto-approve` 开关（防回归） | 通过（自动化 tests/v0_3.rs::tc021a_cli_auto_approve_flag_exists） |
| TC-021b | REQ-015 | 单元 | `ConsoleApprover` 公开导出 | 构造 `ConsoleApprover::new()` | 可构造（CLI 交互式 Human Approval 入口存在）；stdin 交互（y/N 按 `action@target` 缓存）属运行时行为，需人工验证 | 通过（自动化 tests/v0_3.rs::tc021b_console_approver_constructible） |

### 4.3.3 V0.5 测试契约（Memory 沉淀 / Agent 自治基础，REQ-016）
| 用例ID | REQ | 类型 | Given | When | Then（边界判定） | 状态 |
|--------|-----|------|-------|------|------------------|------|
| TC-022a | REQ-016 | 单元 | `RuntimeKernel` / `LibSQLStorage` | `record_memory("action.retain", ...)` 调用两次同标签 | 首次 hits=1；再次 hits=2 且 payload 更新（upsert 累加）；`recall_all`/`recall_memory` 命中，`recall_memory("missing")` 为 None | 通过（自动化 tests/v0_3.rs::tc022a_memory_store_roundtrip） |
| TC-022b | REQ-016 | e2e | `examples/crm.odl`（`Customer.actions.retain`）经 registry 跑两轮 | 第一轮跑通后 `recall_all` 含 `action.retain`；第二轮同 kernel 累加 hits≥2 并产出 `MemoryRecall` 认知事件 | Action→Memory→下次 Plan 闭环可见；hits 随轮次累加（自学习沉淀） | 通过（自动化 tests/v0_3.rs::tc022b_planner_memory_closed_loop） |

### 4.4 异常与边界用例（必须覆盖）
| 用例ID | 关联 | 场景 | 预期 | 状态 |
|--------|------|------|------|------|
| TC-001-b | REQ-001 | .odl 缺 property / 未知类型 | 返回明确错误码，不 panic | 通过（自动化） |
| TC-007-b | REQ-007 | 事件缺 `object_refs` | 写入拦截，返回 WB2006，不落库 | 通过（自动化） |
| TC-010-b | REQ-010 | Pi 进程未就绪 / 崩溃 | 触发运行返回 WB2004，运行置 Failed | 通过（自动化 tests/v0_3.rs） |
| TC-011-b | REQ-011 | Policy 拒绝某 Action | 返回 WB2005，等待 Human Approval | 通过（自动化 tests/v0_3.rs） |
| TC-012-b | REQ-012 | 重复建立相同边 (src, edge, dst) | 返回 WB2007，不重复落库 | 通过（自动化） |

## 5. 风险点 & 注意事项
- 异常分支遗漏是线上 bug 高发区，4.3 不得省略。
- 用例与 REQ 漂移会导致边界失守；REQ 变更须同步更新本表与 acceptance-criteria。

## 6. 关联文档
- [../02_需求设计/requirement-all.md](../02_需求设计/requirement-all.md)
- [../02_需求设计/acceptance-criteria.md](../02_需求设计/acceptance-criteria.md)
- [../07_测试文档/test-plan.md](../07_测试文档/test-plan.md)
- [../07_测试文档/bug-record-template.md](../07_测试文档/bug-record-template.md)
- [../09_迭代与版本/task-spec-template.md](../09_迭代与版本/task-spec-template.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成 |
| V2.0 | lushaohui | 2026-08-11 | 重写为测试契约表：TC-001~011 与 REQ 一一对应（V0.1/V0.3 分组），Given-When-Then + 异常边界用例 |
| V2.1 | lushaohui | 2026-08-11 | 表名对齐 table-design 约束：`trace_event` → `wb_trace_event` |
| V2.2 | lushaohui | 2026-08-11 | 补 V0.2 测试契约：TC-012(REQ-012 Relation)/TC-013(REQ-013 领域 Event)/TC-012-b(重复边 WB2007)；章节重排 4.2=V0.2 / 4.3=V0.3 / 4.4=异常 |
| V2.3 | lushaohui | 2026-08-11 | TC-012 事件枚举名 ObjectLinked → RelationLinked，对齐 data-dict.md / agent-observation-design.md / api-agent-trace.md 三份权威文档的事件类型枚举；同步代码 runtime.rs 与测试契约 |
| V2.4 | lushaohui | 2026-08-12 | 新增 TC-005-b（旧 schema 打开拒 WB2101，对应 `oaip db reset` 迁移防护）；bin/oaip `db reset` 子命令落表 |
| V2.5 | lushaohui | 2026-08-12 | V0.3=MVP 全绿：TC-009/010/011 + TC-010-b/011-b 状态置「通过（自动化 tests/v0_3.rs）」；补充 4.3/4.4 状态列 |
| V2.6 | lushaohui | 2026-08-12 | V0.3 补充测试：新增 TC-014(多候选闭环缩放)/TC-015(空世界 world 哨兵)/TC-016(TraceQuery 过滤+游标)/TC-017(REST 列表+WB2002)/TC-018(POST /run 门禁+Executive 决策)/TC-019(WS 按 run 隔离)；全部 tests/v0_3.rs 通过 |
| V2.7 | lushaohui | 2026-08-12 | V0.4 完整 Action 引擎落地：新增 4.3.1 节登记 TC-020a/b/c（REQ-014，tests/v0_3.rs::tc014a/b/c 全绿）；`self`/`self.field` 值表达式 + 幂等 creates/link + `action.<name>` 事件断言 |
| V2.8 | lushaohui | 2026-08-12 | Policy 引擎 + 交互式 Human Approval 落地：新增 4.3.2 节登记 TC-015a/b/c/d（REQ-015，policy 解析/审批闸门/前置条件/Planner 闭环）+ TC-021a/b（CLI `--auto-approve` 标志 + `ConsoleApprover` 导出）；全部 tests/v0_3.rs 通过 |
| V2.9 | lushaohui | 2026-08-12 | V0.5 Memory 沉淀落地：新增 4.3.3 节登记 TC-022a/b（REQ-016，记忆存储 upsert 往返 / Planner 两轮闭环召回）；Action→Memory→下次 Plan 闭环可见，hits 随轮次累加；全部 tests/v0_3.rs 通过 |
