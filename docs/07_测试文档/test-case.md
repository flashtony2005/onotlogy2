# 测试用例（测试契约表）
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-07-002 |
| 所属模块 | WorkBuddy-Test |
| 版本 | V2.5 |
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
