# Agent 认知轨迹接口（Agent Trace API）
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-05-004 |
| 所属模块 | OpenAIP-OS-API |
| 版本 | V1.5 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
定义 Agent 认知轨迹（Trace）的查询与实时流接口，支撑 `agent-observation-design.md` 中的三层观察界面（Executive / Operator / Developer View）与 Decision Graph。本文是 MVP 可观察性层（REQ-008/009、REQ-010/011）的接口落地，供前端 `agent-observation-ui` 与任何第三方消费方调用。

## 2. 业务/技术背景
- 数据来源：Agent Runtime 经 Event Bus 写出的 `TraceEvent`（见 `agent-observation-design.md` 4.3、4.7），持久化于 `wb_trace_event` 表（见 `sql-script.md`）。
- 实时通道：WebSocket `/api/v1/wb/agent/{id}/stream` 推送 `TraceEvent`，与离线查询共用同一数据模型。
- 关联：`03_架构设计/agent-observation-design.md`、`03_架构设计/pi-agent-integration.md`、`02_需求设计/requirement-all.md`（REQ-008~011）、`05_接口文档/api-restful-spec.md`、`05_接口文档/error-code.md`。

## 3. 核心规则 & 约束（重点）
- [强制] 接口前缀统一 `/api/v1/wb/agent/`；返回体统一包装 `{ "code": 0, "msg": "success", "data": ... }`。
- [强制] 分页统一参数 `pageNum`、`pageSize`、`total`；所有列表接口必须支持。
- [强制] 每个 Trace 事件必须携带 `objectRefs`（绑定 `object_refs`），缺失视为数据异常（见 WB2006）。
- [强制] WebSocket 推送的 payload 必须与 `TraceEvent` 字段一致，禁止自定义非标字段。
- [强制] 所有写类轨迹接口（触发运行）需 `PolicyCheck` 通过，被拒返回 WB2005。
- [建议] 查询接口对 `runId` 建索引；实时流建议前端做断线重连并补齐历史。

## 4. 详细设计/内容

### 4.1 Trace 事件通用结构（TraceEvent DTO）
```json
{
  "id": "tr_abc123",
  "kind": "cognitive",
  "runId": "run_001",
  "agentId": "retention-agent",
  "timestamp": "2026-08-11T09:01:05Z",
  "eventType": "Reasoning",
  "objectRefs": ["Customer_001", "Order_005"],
  "reasoning": "Risk increased because payment delay",
  "input": { "goal": "reduce churn" },
  "output": { "intent": "Customer Retention", "confidence": 0.93 },
  "confidence": 0.93
}
```
`kind=world` 时 `runId/agentId/reasoning` 等可空。`eventType` 枚举（对应 `TraceType`）：world 类 `ObjectCreated` / `ObjectUpdated` / `ObjectDeleted` / `RelationLinked` / `DomainEvent`；cognitive 类 `Goal` / `Intent` / `OntologyQuery` / `Reasoning` / `Planning` / `PolicyCheck` / `Action` / `MemoryUpdate` / `Reflection`（定义见 `agent-observation-design.md` 4.3）。

### 4.2 分页查询 Trace 事件列表
- [强制] 请求方式：GET
- [强制] 接口路径：/api/v1/wb/agent/trace
- [强制] 权限要求：登录用户（Operator / Developer 角色可读）
- 请求参数（Query）：

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| runId | string | 否 | 按 Agent 运行 ID 过滤 |
| agentId | string | 否 | 按 Agent ID 过滤 |
| eventType | string | 否 | 按事件类型过滤（枚举见 4.1） |
| objectRef | string | 否 | 按绑定对象 ID 过滤（如 Customer_001） |
| pageNum | int | 是 | 页码，从 1 起 |
| pageSize | int | 是 | 每页大小 |

- 响应成功示例：
```json
{
  "code": 0, "msg": "success",
  "data": {
    "total": 2,
    "list": [
      { "id": "tr_abc123", "runId": "run_001", "agentId": "retention-agent",
        "timestamp": "2026-08-11T09:01:05Z", "eventType": "Reasoning",
        "objectRefs": ["Customer_001"], "confidence": 0.93 }
    ]
  }
}
```
- 响应失败错误码：WB1001（参数错误）、WB1002（未登录）、WB2001（Agent 不存在）
- 特殊逻辑：按 `timestamp` 升序；`objectRef` 反查经 `wb_ontology_object.id`（`object_refs` 存对象ID数组，如 `customer_001`）；结果支持流式分批拉取

### 4.3 获取单个 Trace 事件
- [强制] 请求方式：GET
- [强制] 接口路径：/api/v1/wb/agent/trace/{id}
- [强制] 权限要求：登录用户
- 请求参数（Path）：`id`（Trace 事件 ID）
- 响应成功示例：
```json
{ "code": 0, "msg": "success", "data": { "id": "tr_abc123", "runId": "run_001", "eventType": "Reasoning", "objectRefs": ["Customer_001"], "reasoning": "Risk increased because payment delay", "confidence": 0.93 } }
```
- 响应失败错误码：WB1002（未登录）、WB2002（Trace 不存在）
- 特殊逻辑：无

### 4.4 获取 Agent 运行时间线
- [强制] 请求方式：GET
- [强制] 接口路径：/api/v1/wb/agent/run/{runId}/timeline
- [强制] 权限要求：登录用户（Executive 角色仅可见 Mission/Progress/Decision 摘要）
- 请求参数（Path）：`runId`；可选 Query：`view`（executive | operator | developer，默认 operator）
- 响应成功示例：
```json
{
  "code": 0, "msg": "success",
  "data": {
    "runId": "run_001", "agentId": "retention-agent", "status": "Running",
    "progress": 70, "humanApproval": "Required",
    "events": [
      { "id": "tr_001", "eventType": "Goal", "timestamp": "2026-08-11T09:01:02Z", "confidence": 0.93 },
      { "id": "tr_002", "eventType": "Reasoning", "timestamp": "2026-08-11T09:01:05Z", "objectRefs": ["Customer_001"] }
    ]
  }
}
```
- 响应失败错误码：WB1002（未登录）、WB2003（运行不存在）
- 特殊逻辑：`view=executive` 时 `events` 仅返回 Goal/Decision/Risk 摘要，隐藏 Reasoning 细节

### 4.5 触发 Agent 运行
- [强制] 请求方式：POST
- [强制] 接口路径：/api/v1/wb/agent/run
- [强制] 权限要求：登录用户（Developer / Operator 可触发；受 Policy 约束）
- 请求参数（Body）：

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| agentId | string | 是 | 目标 Agent（如 retention-agent） |
| goal | string | 是 | 自然语言目标，经 Pi（Cognitive Engine）NLU 解析为 Intent |
| objectRefs | array | 否 | 初始作用对象（如 ["Customer_001"]） |
| context | object | 否 | 附加上下文 |

- 响应成功示例：
```json
{ "code": 0, "msg": "success", "data": { "runId": "run_001", "status": "Running", "streamUrl": "/api/v1/wb/agent/run_001/stream" } }
```
- 响应失败错误码：WB1001（参数错误）、WB1002（未登录）、WB2004（认知引擎不可用，Pi 未就绪）、WB2005（Policy 拒绝）
- 特殊逻辑：执行流 `用户输入 → Pi(NLU) → Intent → Ontology Query → Planner → Action → Executor → Trace UI`（见 `pi-agent-integration.md` 4.3）；触发后实时经 WebSocket 推送；幂等按 `runId` 去重

### 4.6 WebSocket 实时轨迹流
- [强制] 协议：WebSocket
- [强制] 接口路径：/api/v1/wb/agent/{id}/stream（其中 `{id}` 为 `runId` 或 `agentId`）
- [强制] 权限要求：登录用户（订阅需与 runId/agentId 权限匹配）
- 推送消息（payload 与 4.1 一致）：
```json
{
  "id": "tr_abc123", "runId": "run_001", "agentId": "retention-agent",
  "timestamp": "2026-08-11T09:01:05Z", "eventType": "Reasoning",
  "objectRefs": ["Customer_001"], "reasoning": "Risk increased because payment delay",
  "confidence": 0.93
}
```
- 失败错误码（握手阶段 HTTP 状态）：401（未登录 WB1002）、404（运行/ Agent 不存在 WB2001/WB2003）
- 特殊逻辑：连接建立后推送历史全量事件，再增量推送；断线客户端应在 `onclose` 后以 `?after={lastEventId}` 重连补齐；单连接超时建议 30 分钟

### 4.7 字段命名与序列化约定
- [强制] 存储层（Rust 模型 / Turso）字段为 snake_case（`object_refs` / `run_id` / `event_type`）；API JSON 为 camelCase（`objectRefs` / `runId` / `eventType`），经 serde `rename_all = "camelCase"` 自动映射，禁止手写字面量。
- [强制] `id` 两端一致：DB 为 TEXT 语义ID（如 `tr_abc123` / `customer_001`），API DTO 的 `id` 即该值，无额外编码/解码映射。
- [强制] `objectRefs`（API）/ `object_refs`（DB）存对象ID数组，反查经 `wb_ontology_object.id`；禁止引用不存在的表（原 `object_context` 已废弃）。

### 4.8 获取 Agent 记忆沉淀
- [强制] 请求方式：GET
- [强制] 接口路径：/api/v1/wb/agent/memory
- [强制] 权限要求：登录用户（Operator / Developer 角色可读）
- 请求参数：无
- 响应成功示例：
```json
{
  "code": 0, "msg": "success",
  "data": {
    "total": 2,
    "list": [
      { "label": "action.retain", "payload": "{\"hint\":\"offer discount\"}", "hits": 5, "createdAt": "2026-08-12T09:01:05Z", "updatedAt": "2026-08-12T09:01:05Z" }
    ]
  }
}
```
- 响应失败错误码：WB1002（未登录）
- 特殊逻辑：数据来源 `wb_memory`（V0.5·REQ-016 记忆沉淀）；`list` 按 `hits` 降序，是观察 UI Memory 视图的数据源；经验经 `MemoryRecall` 认知事件在下一轮 run 召回，形成 Action→Memory→下次 Plan 闭环

### 4.9 获取 Evolution 状态（Agent 自学习）
- [强制] 请求方式：GET
- [强制] 接口路径：/api/v1/wb/agent/evolution
- [强制] 权限要求：登录用户（Developer 角色可读，含可追溯证据链）
- 请求参数：无
- 响应成功示例：
```json
{
  "code": 0, "msg": "success",
  "data": {
    "patterns": [
      { "label": "pattern.action.retain", "action": "retain", "support": 5, "confidence": 1.0, "runIds": ["RUN-1","RUN-2"] }
    ],
    "proposals": [
      { "id": "proposal.retain", "kind": "ActionPattern", "target": "retain", "confidence": 1.0, "status": "Validated", "patternLabel": "pattern.action.retain", "runIds": ["RUN-1","RUN-2"] }
    ],
    "learned": [
      { "id": "learned.retain", "kind": "ActionPattern", "action": "retain", "confidence": 1.0, "proposalId": "proposal.retain", "runIds": ["RUN-1","RUN-2"], "createdAt": "2026-08-12T09:02:00Z" }
    ]
  }
}
```
- 响应失败错误码：WB1002（未登录）
- 特殊逻辑：数据来源 `wb_pattern` / `wb_proposal` / `wb_learned`（BL-003·REQ-017 Evolution Engine）；`run_evolution` 幂等（不重复产出知识），`run_ids` 跨 run 合并去重。**Learn≠Permission：本端点仅暴露知识，不提供任何改写 Policy / 绕过 Human Approval（WB2005）的能力**。AutoApprove 类「Knowledge→Permission」升级由 **REQ-018** 落地（5 道闸门 + 默认关闭 + 可审计 + 可撤销，详见 4.9.1 / 4.9.2）。`runIds` / `patternLabel` / `proposalId` 构成可追溯证据链（LearnedKnowledge→Proposal→Pattern→Experience→AgentRun）

### 4.9.1 切换 LearnedKnowledge 自动放行资格（REQ-018 · 可撤销）
- [强制] 请求方式：POST
- [强制] 接口路径：/api/v1/wb/agent/evolution/learned/{id}/auto-approve
- [强制] 权限要求：登录用户（Developer 角色可写）
- 请求体（JSON）：
```json
{ "enabled": true }
```
- 响应成功示例：
```json
{ "code": 0, "msg": "success", "data": { "id": "learned.refund", "autoApproveEnabled": true } }
```
- 特殊逻辑：REQ-018 可撤销——**幂等**切换 `LearnedKnowledge.auto_approve_enabled`（不删除知识）；`enabled=false` 后该 action 下次命中 5 道闸门时因 `auto_approve_enabled=false` 立即回退 Human Approval；切换同时写 `wb_approval_audit`（decision=`auto_approve_enabled` | `revoke_disabled`）。需先经 `--auto-approve-evolution` 开启全局开关，否则 `auto_approve_enabled` 不会被 `may_auto_approve` 采纳

### 4.9.2 列出自动放行审计（REQ-018 · 可审计）
- [强制] 请求方式：GET
- [强制] 接口路径：/api/v1/wb/agent/evolution/audit
- [强制] 权限要求：登录用户（Developer 角色可读）
- 响应成功示例：
```json
{
  "code": 0, "msg": "success",
  "data": [
    { "id": "audit_2026-08-12T09:02:00Z_0", "runId": "run_001", "action": "refund",
      "learnedId": "learned.refund", "decision": "auto_approve", "reason": "5 道闸门全过，替代一次 Human Approval", "createdAt": "2026-08-12T09:02:00Z" }
  ]
}
```
- 特殊逻辑：数据来源 `wb_approval_audit`（REQ-018 新增）；每次自动放行（decision=`auto_approve`）或资格切换（decision=`auto_approve_enabled` | `revoke_disabled`）均落库，可追溯至 `LearnedKnowledge` + `runId`

### 4.10 列出某运行的审批请求
- [强制] 请求方式：GET
- [强制] 接口路径：/api/v1/wb/agent/run/{runId}/approvals
- [强制] 权限要求：登录用户（Operator / Developer 可读）
- 请求参数（Path）：`runId`
- 响应成功示例：
```json
{
  "code": 0, "msg": "success",
  "data": [
    { "reqId": "apr_001", "runId": "run_001", "agentId": "retention-agent",
      "stepId": "action.retain@customer_001", "description": "需审批：retain@customer_001", "state": "Pending" }
  ]
}
```
- 响应失败错误码：WB1002（未登录）、WB2003（运行不存在）
- 特殊逻辑：返回该运行全部审批请求（含历史裁决，state=Pending/Approved/Rejected/**Expired**）；动作经 `require_approval` 闸门触发时由 `ServerApprover` 挂起运行并写入一条 Pending 请求，前端轮询本接口渲染审批列表（V0.4 遗留项：server 挂起式 HTTP 审批）。**Expired** 表示挂起超时（默认 300s）后由 `ApprovalStore::expire` 回收，运行已按拒绝回退；Expired 与人工 Rejected 状态可区分，前端不得把 Expired 渲染为可裁决的待审批项

### 4.11 裁决一条审批请求
- [强制] 请求方式：POST
- [强制] 接口路径：/api/v1/wb/agent/run/{runId}/approval/{reqId}
- [强制] 权限要求：登录用户（Operator / Developer 可裁决）
- 请求参数（Path）：`runId`、`reqId`；请求体（JSON）：
| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| decision | string | 是 | `approve`/`yes`/`true` → 批准并恢复挂起运行；其余值 → 拒绝，触发 WB2005 终止 |

- 响应成功示例（批准）：
```json
{ "code": 0, "msg": "success", "data": { "reqId": "apr_001", "decision": "Approved" } }
```
- 响应失败错误码：WB1001（参数错误）、WB1002（未登录）、WB2003（运行不存在）、**404（审批请求不存在或已裁决，重复裁决被拒）**
- 特殊逻辑：裁决后置 `ApprovalStore` 中状态为 Approved/Rejected 并唤醒挂起的 Planner；批准 → 运行继续，拒绝 → 动作以 WB2005 终止该运行。**同一 reqId 仅可裁决一次**，重复裁决返回 404（code 字段，非 HTTP status）。与 CLI `ConsoleApprover`、server `--auto-approve` 开关等价，是 Human-in-the-loop 的审批态之一。**超时回收（Expired）的请求同样拒绝裁决**（返回 404），避免超时残留请求被误裁决、UI 出现幽灵待审批。超时与人工拒绝通过 state（Expired/Rejected）区分，事件 `ApprovalDecision.decision` 对应 `Expired`/`Rejected`/`Approved`

## 5. 风险点 & 注意事项
- Trace 缺失 `objectRefs` 会破坏可解释性承诺，写入侧须评审卡点（见 `agent-observation-design.md` 5）。
- WebSocket 断线未重连会导致 Observation UI 出现空洞时间线，前端必须实现补齐逻辑。
- 实时流与离线查询若字段漂移，三层视图会出现不一致，须以 4.1 DTO 为单一真相源。
- 认知引擎（Pi）未就绪时触发运行应快速失败（WB2004），避免悬挂连接。

## 6. 关联文档
- [../05_接口文档/api-restful-spec.md](../05_接口文档/api-restful-spec.md)
- [../05_接口文档/error-code.md](../05_接口文档/error-code.md)
- [../03_架构设计/agent-observation-design.md](../03_架构设计/agent-observation-design.md)
- [../03_架构设计/pi-agent-integration.md](../03_架构设计/pi-agent-integration.md)
- [../02_需求设计/requirement-all.md](../02_需求设计/requirement-all.md)
- [../02_需求设计/prd-agent-integration.md](../02_需求设计/prd-agent-integration.md)

### 4.12 获取评测结果（Phase 11 · Domain Agent Evolution）
- [强制] 请求方式：GET
- [强制] 接口路径：/api/v1/wb/agent/evaluation
- [强制] 权限要求：登录用户（Operator 及以上可读；写由 Runtime RunEnd 钩子独占）
- 请求参数：`runId`（可选，按 run 过滤）
- 响应成功示例：
```json
{
  "code": 0, "msg": "success",
  "data": {
    "total": 1,
    "list": [
      {
        "runId": "RUN-1",
        "taskId": "t1",
        "correctness": { "value": 0.8, "confidence": 1.0, "evidence": [ { "seq": 4, "eventType": "test.result" } ] },
        "completeness": { "value": null, "confidence": 0.0, "evidence": [] },
        "efficiency": { "value": 0.5, "confidence": 1.0, "evidence": [ { "seq": 3, "eventType": "worker.completed" } ] },
        "reliability": { "value": 1.0, "confidence": 1.0, "evidence": [ { "seq": 2, "eventType": "worker.started" } ] },
        "policyCompliance": { "value": 1.0, "confidence": 1.0, "evidence": [ { "seq": 0, "eventType": "policy.checked" } ] },
        "outputQuality": { "value": null, "confidence": 0.0, "evidence": [] },
        "aggregate": 0.825,
        "unknowns": ["completeness", "outputQuality"],
        "failureModes": [],
        "evaluatorVersion": "11.1.0"
      }
    ]
  }
}
```
- 特殊逻辑：数据来源 `wb_evaluation`（**派生表**——Evaluation = Pure Function(Event Log)，删表可经 Evaluation Reducer 重建）；**Unknown ≠ Success**：无证据维度 value=null（aggregate 剔除）；judge 维度 confidence 来自 judge.submitted 事件；写路径只有 RunEnd 钩子（`on_run_end`），本端点只读


## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成：Trace 列表/详情/时间线/触发运行/WebSocket 流接口，统一 AgentTraceEvent DTO |
| V1.1 | lushaohui | 2026-08-11 | 方案B对齐：DTO 泛化为 TraceEvent（补 kind，区分 world/cognitive）；表引用 agent_trace_event→trace_event |
| V1.2 | lushaohui | 2026-08-11 | 表引用统一对齐 table-design 约束，改为 wb_trace_event |
| V1.3 | lushaohui | 2026-08-11 | 修正 object_context 反查为 wb_ontology_object.id；新增 4.7 字段命名与序列化约定（snake_case↔camelCase + id 语义ID一致） |
| V1.4 | lushaohui | 2026-08-12 | 新增 4.8 GET /api/v1/wb/agent/memory（wb_memory 记忆沉淀视图源）、4.9 GET /api/v1/wb/agent/evolution（wb_pattern/wb_proposal/wb_learned Evolution 状态，含可追溯证据链与 Learn≠Permission 约束） |
| V1.5 | lushaohui | 2026-08-12 | REQ-018 Evolution 第二阶段 AutoApprove 接口：4.9.1 POST /evolution/learned/{id}/auto-approve（可撤销切换 auto_approve_enabled，幂等）；4.9.2 GET /evolution/audit（wb_approval_audit 审计列表，可追溯至 LearnedKnowledge+run）；4.9 特殊逻辑补 REQ-018 升级已落地说明 |
| V1.5 | lushaohui | 2026-08-12 | 新增 4.10 GET /run/:run_id/approvals 与 4.11 POST /run/:run_id/approval/:req_id（V0.4 挂起式 HTTP 审批端点，Human-in-the-loop 三态之一；重复裁决返回 404 code） |
| V1.6 | lushaohui | 2026-08-15 | 新增 4.12 GET /api/v1/wb/agent/evaluation（Phase 11 · 11A 证据系统：每维度 Score{value,confidence,evidence} + unknowns + aggregate；派生表语义，Event Log 永远是 Source of Truth；Operator 及以上可读） |
### 4.13 Mutation 提案（Phase 11B 诊断环）

#### 4.13.1 列出 Mutation 提案
- [强制] 请求方式：GET
- [强制] 接口路径：/api/v1/wb/agent/mutation
- [强制] 权限要求：登录用户（Operator 及以上可读）
- 请求参数：`status`（可选：PROPOSED/VALIDATED/EXPERIMENTAL/ACTIVE/REJECTED/ARCHIVED）
- 响应成功示例：
```json
{
  "code": 0, "msg": "success",
  "data": {
    "total": 1,
    "list": [
      {
        "id": "mutation.fix-java-bug.v1+missing-test-evidence",
        "targetAsset": "fix-java-bug",
        "sourceVersion": "v1",
        "candidateVersion": "v1+missing-test-evidence",
        "rootCause": "执行路径没有产生 TestResult / relation / judge 证据——",
        "evidence": ["run_1", "run_2"],
        "delta": { "insert_step": { "step": "run_tests" } },
        "status": "PROPOSED",
        "promotionReason": null
      }
    ]
  }
}
```
- 特殊逻辑：数据来源 `wb_mutation`；诊断环自动产出提案（CLI run 尾部 / 显式调用），**状态 PROPOSED 等待人工评审**（Proposal ≠ Mutation）

#### 4.13.2 人工评审 Mutation 提案
- [强制] 请求方式：POST
- [强制] 接口路径：/api/v1/wb/agent/mutation/{id}/review
- [强制] 权限要求：登录用户（Developer 角色可写）
- 请求体（JSON）：
```json
{ "decision": "approve", "reason": "可选，仅 reject 时记录" }
```
- 响应成功示例（approve）：
```json
{
  "code": 0, "msg": "success",
  "data": {
    "id": "mutation.fix-java-bug.v1+missing-test-evidence",
    "status": "VALIDATED",
    "promotionReason": "human approved by admin; apply failed: 基准 Skill 不存在：fix-java-bug（请人工细化 Delta）",
    "apply": { "candidateId": "fix-java-bug@v1+missing-test-evidence", "status": "EXPERIMENTAL" }
  }
}
```
- 特殊逻辑：`approve` → PROPOSED→VALIDATED（评审记录进 promotion_reason，可审计），并尝试 **Apply**：结构化 Delta 应用到基准 Skill（Fork）→ 注册 EXPERIMENTAL 候选（原资产保留）；基准不存在 / Delta 引用了不存在的步骤 → apply 失败但评审状态不受影响（提示人工细化）。`reject` → PROPOSED→REJECTED（失败保留不删除）。已评审提案不会被诊断环重新生成（否决决定持久）。Learn ≠ Permission：评审（人工）才是从提案到应用的唯一通道
