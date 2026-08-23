# 性能设计
## 基本信息
| 项 | 内容 |
|----|------|
| 文档ID | WB-DOC-03-004 |
| 所属模块 | OpenAIP-OS-Architecture |
| 版本 | V1.1 |
| 创建人 | lushaohui |
| 创建时间 | 2026-08-11 |
| 更新记录 | 见文末修订日志 |
| 生效环境 | 开发/测试/预发/生产 |

## 1. 文档目的
定义性能目标、并发模型、缓存与压测策略。

## 2. 业务/技术背景
依赖 architecture-total.md 与 module-split.md。

## 3. 核心规则 & 约束（重点）
- [强制] 必须给出核心接口的性能指标（QPS、P99 延迟）。
- [建议] 缓存策略需说明失效与一致性方案。

## 4. 详细设计/内容
### 4.1 性能目标
MVP 为 Local-First 单体（Rust + Turso 嵌入式，无网络往返），目标按单节点设定：

| 接口/场景 | QPS | P99 延迟 | 并发 |
|-----------|-----|---------|------|
| ObjectEngine 本地 CRUD（`oaip object list` / query） | 5,000 | < 10 ms | 单进程 |
| `GET /api/v1/wb/agent/trace`（Trace 列表分页） | 200 | < 50 ms | 50 |
| `GET /api/v1/wb/agent/run/{id}/timeline` | 100 | < 80 ms | 50 |
| WebSocket 实时轨迹流（`/api/v1/wb/agent/{id}/stream`） | - | 推送延迟 < 100 ms | 1,000 连接 |
| `oaip compile *.odl`（单文件编译 + Codegen） | - | < 200 ms | 单进程 |

### 4.2 缓存策略
- **ObjectDef 元数据缓存**：编译期生成的 `ObjectDef`（含字段 schema）常驻内存 LRU 缓存，运行时由 `ObjectDef` 驱动行为；schema 变更（重新 compile）即失效整块缓存，避免脏读。
- **World Model 聚合缓存**：V0.2 起 `query_world()` 结果按 `(type, id)` 缓存 5s（对象图变化不频繁），失效由写操作主动驱逐。
- **Trace 查询不缓存**：`wb_trace_event` 为 append-only 审计流，查询直打索引（idx_trace_run / idx_trace_type / idx_trace_time），保证观察与持久化同源。
- **一致性**：缓存均为「写穿透 + 主动失效」，不存在最终一致性窗口外的读旧；Turso 为嵌入式单写者，无跨节点缓存一致性问题。

### 4.3 压测方案
- **API / WS**：用 `oha`（HTTP）与 `k6`（WebSocket）对单节点施压，基线取 P99 与错误率；目标 P99 达标且错误率 0。
- **本地关键路径**：用 `criterion` 对 ODL 编译、投影重放、世界查询、审计写入和推理分类做 Rust 基准；执行入口为 `make bench`，首轮数字见 [performance-baseline.md](performance-baseline.md)。
- **火焰图**：`cargo flamegraph` 定位 compile / query 瓶颈，禁止在热路径做 JSON 全表扫描。
- **fail-fast**：认知引擎（Pi）未就绪时触发运行应毫秒级返回 WB2004，避免连接悬挂拖垮并发。


## 5. 风险点 & 注意事项
- 缓存与数据库一致性是常见坑，需明确失效策略。

## 6. 关联文档
- [../03_架构设计/architecture-total.md](../03_架构设计/architecture-total.md)
- [../08_部署运维/monitor-alert.md](../08_部署运维/monitor-alert.md)

## 修订日志
| 版本 | 修改人 | 修改日期 | 修改内容 |
|------|--------|----------|----------|
| V1.0 | lushaohui | 2026-08-11 | 初稿完成 |
| V1.1 | lushaohui | 2026-08-11 | 填充 4.1 性能目标（MVP 单节点基线）、4.2 缓存策略（ObjectDef/WorldModel 缓存 + Trace 不缓存）、4.3 压测方案 |
| V1.2 | lushaohui | 2026-08-22 | 增加 Criterion 四路径基线入口与首轮结果链接 |
| V1.3 | lushaohui | 2026-08-23 | 增加不可变推理快照的首次/缓存分类基准与失效边界 |
