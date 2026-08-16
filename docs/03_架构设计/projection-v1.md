# Projection V1 Contract — Storage → Kernel 语义投影

| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-03-P5-PROJECTION |
| 状态 | 契约定稿（2026-08-16） |
| 前置 | architecture-v2.md（I1：State = Reduce(EventLog)） |

## 0. 一句话

**Projection 是 Event Log 的纯函数**：`wb_trace_event`（world 事实）→ 确定性规则 →
Kernel Relation 更新 → World State。Run 不是事实入口——Event 才是。

```
Storage Event ──► Projection Rule ──► Kernel Mutation ──► Relation Update ──► World State
     │                                                                           │
     └────────────── Event Log（唯一事实源）◄────────────────────────────────────┘
```

## 1. 输入与有序性

- 输入：`wb_trace_event` 中 `kind=World` 的事件流（含领域事件）。
- 序：`(timestamp, id)` 字典序（与 TraceQuery after_id 游标同源）。
- Projection 无视 `run_id`——事实不依赖 Run 容器（审计解耦点）。

## 2. 事件载荷约定（Projection 契约的一部分）

| 事件类型 | object_refs | input |
|----------|-------------|-------|
| `ObjectCreated` | `[id]` | `{"type": T, "data": {字段...}}` |
| `ObjectUpdated` | `[id]` | `{"data": {字段...}}`（整体替换该对象的关系投影） |
| `ObjectDeleted` | `[id]` | 无 |
| `RelationLinked` | `[source, target]` | `{"edge": E}` |
| `RelationUnlinked` | `[source, target]` | `{"edge": E}` |
| 领域事件（注册制） | `[subject]` | `{"to": value}`（规则：event_type → predicate） |

## 3. 投影规则（确定性、Metadata 驱动）

1. `ObjectCreated`：relate `(id, "type", T)` + 每个字段 `(id, field, value)`。
   幂等：`(id, "type", T)` 已 current 则整体跳过。
2. `ObjectUpdated`：在事件时刻 expire 该 id 的全部 current 关系（valid_until = 事件
   时间戳），再按 `{"data"}` 重新 relate（含 type 保留语义：data 不含 type 时保持原 type 关系）。
   实现注：为确定性，先重 relate 再 expire 旧值，或 expire-at-then-relate 均可，
   以「重放得到相同 (source,predicate,target,valid_from,valid_until) 元组集」为准。
3. `ObjectDeleted`：expire 该 id 全部 current 关系（source 侧与 target 侧）。
4. `RelationLinked`：relate `(refs[0], edge, refs[1])`；已 current 则跳过（幂等）。
5. `RelationUnlinked`：expire 匹配 `(refs[0], edge, refs[1])` 的 current 边。
6. 领域规则：`with_domain_rule(event_type, predicate)` 注册后，
   `(refs[0], predicate, input.to)`。示例：`OrderPlaced → (order, "status", "placed")`。

字段值 stringify：标量 → `to_string()`（数字 "5"、布尔 "true"）；对象/数组 → JSON 文本。
type 关系保留字为 `"type"`，字段不得占用（契约约束）。

**已知限制（P5 内接受）**：字段投影为文本后丢失类型（`(id,"amount","5")` 无法区分
数字 5 与字符串 "5"）。compose Precondition 是 Value 比较——策略判定如需数值语义，
由 world/entity.rs 的 EntityView 负责还原（类型保持的实体视图），在 P5.5 接入闭环时
一并落地；在此之前投影世界仅用于观察与决策输入，数值型前置条件的领域规则需
显式注册（如 `amount` 规则先做文本比较的约定）。

## 4. 幂等与重放不变式（验收条件）

```
P1  Replay：Projection(E1..En) 的关系元组集 == Replay(E1..En) 的关系元组集
    （同一事件流，全新 Kernel 全量重放，得到完全相同 World State）
P2  幂等：对同一事件（同 id）再次应用，状态不变
     Projection(E1..En + En) == Projection(E1..En)
P3  增量：Projection 维护 watermark（最后应用事件 id）；project() 只应用 id 之后的事件
P4  确定性：规则只用事件载荷与时间戳，不用墙钟（expire 时刻 = 事件时间戳）
P5  写入约束：EveryWorldChange → Event——对象/关系写入必须伴随对应 world 事件，
    且对象写与事件写在同一事务（libsql 单连接事务），杜绝半写不一致
P6  闭环约束：Effect 提交（step Committed）产生的 Kernel 世界变化必须回写
     world 事件（ObjectUpdated/RelationLinked 形式）——Event Log 才完整，
     P1 重放才包含 Runtime 造成的变化（两个 Kernel 写入者收敛为单向流）
```

实现机制：watermark 保证 P2/P3；时间戳作为 valid_from/valid_until 保证 P1/P4；
P5/P6 是写入路径的强约束（CLI/runner/server 三处接线时落实）。

> 命名说明：投影不变式用 P1-P6，避免与 Kernel 不变量矩阵（I1-I10，
> tests/invariants.rs）撞名——内核 I2「Projection ∉ EventLog」指假设轨不写日志，
> 与本层 P1-P6 语义不同。

## 5. 边界（本阶段不做）

- 不做完整 Ontology 编译映射（ODL → 投影规则自动生成）：规则先用显式约定 + 注册制。
- 不做 Temporal 查询（as_of 属于世界模型后续阶段；Kernel P3 只保证 current）。
- 不做双向同步：Storage 是事实源，Kernel 是投影视图（只读方向）。

## 5.5 触发时机（评审定案）

- **P5.5 阶段**：per-run 全量投影——CLI `oaip agent run` 与 server `POST /run` 在
  新建 Kernel 后、`run_agent` 前，把事件日志全量投影进 Kernel（正确性优先；
  server 每次 run 重建 Kernel 是现有架构事实，投影随之重建）。
- **后续里程碑**：跨 run 常驻 Kernel + watermark 增量投影（性能问题，非正确性问题）。

## 6. 接入闭环（P5.5 验收）

```
oaip run（写对象 + ObjectCreated 事件）
  → wb_trace_event（事实源）
      → Projection
          → Kernel World（Agent 第一次看见 Storage 世界）
              → AgentLoop observe/decide/act
                  → 新事件 → 投影更新
```

## 6.5 审计解耦（P5.6 · 现状澄清与目标）

现状：审计事件（approval.* / evolution.gate_approved）已在 wb_trace_event
（M2 完成），`/audit` 读事件表；但桥接发生在 run 终局（run_agent bridge_audit），
进程崩溃时未终局 run 的 Kernel 审计事件丢失。

P5.6 目标：审计事件**产生时即落库**（Kernel record_event 同步 append 到事件日志），
不依赖 Run 生命周期——Event Log 成为 Audit / Projection / Evaluation / Replay 的
单一扇出源。

## 修订日志

| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-16 | 契约定稿：载荷约定 / 六条规则 / I1-I4 验收 |
