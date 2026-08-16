# M1b · G9 测试迁移矩阵 — v0_3 系测试处置清单

| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-09-006 |
| 所属模块 | OpenAIP-OS-TechDebt |
| 版本 | V1.0 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-16 |
| 前置 | m1-gap-analysis.md（G9） |
| 状态 | 定稿（M1d 删除依据） |

## 1. 目的

v0_3 系 6 个测试文件（63 例）全部依赖 V1 门面（RuntimeKernel/Planner），M1d 随门面删除。
本矩阵逐例给出处置：**已由 V2 测试覆盖**（列等价测试）/ **随 M1d 删除**（V1 专属行为）/
**M1c 重写**（server 接线后行为不变）。M1d 删除时以本矩阵为卡点，缺覆盖即卡回。

## 2. 逐文件处置

### tests/v0_3_agent.rs（27 例）

| v0_3 用例 | 行为 | 处置 | V2 等价 |
|-----------|------|------|---------|
| tc011_policy_allow_non_destructive | Policy Allow 非破坏 | 已覆盖 | compose_v2 tc_c5 / g1a PolicyCheck Allow |
| tc011b_policy_deny_wb2005 | Deny→WB2005 | 删除（V1 错误码映射） | g1c PolicyCheck Deny |
| tc014a_odl_parses_actions | ODL actions 解析 | 已覆盖 | ontology_v2 tc_o1（ODL→RuntimeDefinition） |
| tc010b_engine_unavailable_wb2004 | 引擎不可用 | M1c 重写 | g3d/g3f + server 健康门（PiEngine::health） |
| tc014b_action_engine_executes | V1 ActionEngine | 删除（V1 执行引擎） | ontology_v2 tc_o2 / compose 闭环 |
| tc012b_empty_world_object_refs_sentinel | 空世界哨兵 | 已覆盖 | v2_parity g1a assert_binding（world 兜底） |
| tc010_cognitive_trace_with_refs | 认知事件绑定 | 已覆盖 | v2_parity g1a |
| tc009_three_view_filtering | 三层视图 | 已覆盖 | v2_parity g9c（build_timeline 三视图） |
| tc011_planner_closed_loop | 规划闭环 | 已覆盖 | agent_v2 tc_a1 |
| tc013_trace_query_filters | TraceQuery 过滤 | 已覆盖 | v2_parity g9a（storage 层契约） |
| tc012_multi_candidate_closed_loop | 多候选闭环 | 已覆盖 | runtime_v2 tc_s2 多步 |
| tc011_repeated_run_is_idempotent | 重复运行幂等 | 已覆盖 | runtime_v2 tc_s8 幂等 |
| tc014c_planner_via_registry_closed_loop | 注册表闭环 | 已覆盖 | compose_v2 tc_c1 闭环 |
| tc_action_hook_denies_before_backend | Action hook 闸门 | 删除（tool_backend/hooks 随 planner 死） | — |
| tc_planner_extracts_refund_amount_from_goal | V1 intent 抽取 | 删除（V1 NLU） | PiEngine decide（g3a）承担 LLM 决策 |
| tc_planner_manages_task_lifecycle | V1 任务生命周期 | 已覆盖 | kernel_v2 / compose Task·Fiber 生命周期 |
| tc_t1/t2/t3 refund/retain 过滤 | V1 intent 过滤 | 删除（V1 intent） | — |
| tc_task_lifecycle_via_ontology_actions | 本体动作任务 | 已覆盖 | ontology_v2 tc_o7 端到端 |
| tc_planner_filters_actions_by_intent | 意图过滤动作 | 删除（V1 intent） | — |
| tc_t4_no_id_same_as_before | run id 唯一 | 已覆盖 | v2_parity g6c |
| tc_tool_backend_lint_creates_finding | V1 ToolBackend | 删除（tool_backend.rs 随 planner 死） | worker 层替代（worker_v2） |
| tc_rest_trace_list_and_404 | trace REST | M1c 保留（端点在，数据源换 V2 事件流） | g9a |
| tc_rest_run_start_gate | run 入口门 | M1c 重写 | g3f + RBAC（v0_7 保留） |
| tc_ws_run_scoped / tc009_ws_stream_and_reconnect | WS 流 | M1c 重写（V2 事件流） | g9b/g9c + v0_9 重写 |

### tests/v0_3_approval.rs（5 例）→ M1c 重写

| v0_3 用例 | 处置 |
|-----------|------|
| tc023a 挂起批准 / tc023b 拒绝 / tc023c 超时 / tc023d 区分 / tc023e 错 run 拒绝 | HTTP 端点协议保留，存储换 Waiting 注册表（G2 resolve/expire + v2_parity approval 契约）；在 server_v2 风格测试中重写 |

### tests/v0_3_evolution.rs（21 例）

| v0_3 用例 | 处置 | V2 等价 |
|-----------|------|---------|
| tc025a-h（五阶段 Evolution） | 删除（V1 evolution.rs 三表归 M2） | evolution_v2 tc_e1-e8 |
| tc022a memory store 往返 | 已覆盖 | cognitive_persist_v2 tc_cp1 |
| tc022b planner memory 闭环 | 已覆盖 | agent_v2 tc_a3 |
| tc026a-k（AutoApprove 5 闸门） | 删除 | evolution_v2 tc_e5/e6/e8 + v2_parity gate_source |

### tests/v0_3_policy.rs（6 例）

| v0_3 用例 | 处置 | V2 等价 |
|-----------|------|---------|
| tc015a ODL policy 解析 | 已覆盖 | ontology_v2 tc_o1（PolicyDefinition 提取） |
| tc015b/c/d 审批门/前置条件/策略门 | 已覆盖 | compose::policy 单测 + v2_parity g1c |
| tc021a/b CLI 开关 | M1c 保留（CLI 参数面不变，实现换 V2） | g2 DecisionCache（ConsoleApprover 语义） |

### tests/v0_3_server.rs（4 例）

| v0_3 用例 | 处置 |
|-----------|------|
| tc024a memory 端点 / tc_server_tasks 端点 | M1d 删除（面板并入 V2Panel，G10） |
| tc_static_serves_index_and_assets / tc_static_missing_dir | 保留（静态托管与 planner 无关） |

### tests/v0_9_ws_clamp.rs（4 例）→ M1c 重写

WS 钳制行为不变，事件源换 V2 trace sink 落库（g9b/g9c 已钉时间线行为；钳制测试在 M1c server 接线后重写）。

## 3. 保留文件（不随 M1d 删除）

- tests/v0_1.rs / v0_2.rs：ODL/IR/codegen/对象 CRUD/关系边——V1 编译器与世界存储存活。
- tests/v0_7_auth.rs：认证 RBAC 存活。
- tests/v0_8_persistence.rs：wb_run 重定位为 V2 run store（G7），重启恢复语义保留；
  其中 orphan pending→expired 依赖 wb_approval，M1d 时该例随 wb_approval 删除改写为
  Waiting fiber 超时契约（g2c 已覆盖内核语义）。
- tests/bridge_v2.rs：M1c 时改写为 v2_parity/via_workers 风格（g8a 已覆盖链）后删除 V1 版。

## 4. 出口判定

M1d 删除 v0_3 系文件的前提：
1. v2_parity 全绿（当前 32 例）；
2. M1c 重写项（run 门 / HTTP 审批 / WS 钳制）在 server_v2 风格测试中全绿；
3. 本矩阵中「已覆盖」行逐条核对无缺漏。

## 修订日志

| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-16 | 初稿：63 例逐条处置 + 出口判定 |
