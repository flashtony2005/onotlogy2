# 核心路径性能基线

## 基本信息

- 记录日期：2026-08-22
- 版本：V0.14.0 候选
- 场景：Local-First 单进程，嵌入式 libSQL `:memory:`
- 微基准方法：`cargo bench --bench performance`（Criterion；`core_paths` 30 个样本，
  `knowledge_abox` 10 个样本；默认 3 秒预热 / 5 秒测量）
- 环境：Apple M4；10 CPU；16 GiB 内存；macOS 27.0.0；arm64
- 基线更新：2026-08-24 更新 Knowledge ABox consistency、新增 append-only delta chain 和
  8 线程 cached burst；其余数字记录于 2026-08-22。

## 固定负载

| 基准 | 负载 |
|------|------|
| ODL compile | 24 个对象类型，每个对象 1 个 action |
| Projection replay | 1,000 条确定性 world event |
| World query | 500 个持久化对象 |
| Audit write | 每次迭代写 1 条事件 + 1 个哈希链节点 |
| Reasoner classification | 24 个 named class 的固定继承链；对比新建 Reasoner 和同一不可变快照重复查询 |
| Knowledge ABox consistency | 1,000 条 functional data property 断言；对比全量重建、exact-result 缓存和固定 append-only delta chain |
| Knowledge ABox concurrency | 同一 warmed 1,000-fact snapshot 上，8 线程 × 128 次 cached validation；包含线程创建/join |
| HTTP/WS load | 10 次并发 submit/approve（并发 5），3 个 WebSocket 客户端，真实 release 二进制 |

## 基线结果

| 关键路径 | 平均耗时 | 中位数 | 吞吐 | 目标 | 结论 |
|----------|----------|--------|------|------|------|
| ODL parse + codegen | 48.300 µs | 47.905 µs | 约 496,900 objects/s | 单文件 < 200 ms | 达标 |
| Projection replay | 335.503 µs | 335.756 µs | 约 2,980,600 events/s | 无既有目标；建立基线 | 已建档 |
| World object query | 247.964 µs / 批次 | 246.653 µs / 批次 | 约 2,016,400 objects/s | 本地 CRUD < 10 ms | 达标 |
| Audit append + hash chain | 27.961 µs | 27.985 µs | 约 35,764 writes/s | 无既有目标；建立基线 | 已建档 |
| Reasoner classification (new) | 1.358 ms | 约 1.358 ms | 约 17,678 classes/s | 无既有目标；建立基线 | 已建档 |
| Reasoner classification (snapshot cache) | 9.401 µs | 约 9.401 µs | 约 2,553,100 classes/s | 无既有目标；建立基线 | 已建档 |
| Knowledge ABox consistency (cold rebuild) | 972.585 µs | 964.734 µs | 约 1,028,200 assertions/s | 无既有目标；建立基线 | 已建档 |
| Knowledge ABox consistency (exact-result cache) | 34.745 µs | 34.518 µs | 约 28,780,000 assertions/s | 无既有目标；建立基线 | 已建档 |
| Knowledge ABox consistency (append-only delta chain) | 191.292 µs | 189.999 µs | 约 5,228 appends/s | 无既有目标；建立基线 | 约为全量耗时 19.7% |
| Knowledge ABox concurrent cached validation (8×128) | 18.568 ms / burst | 约 18.568 ms / burst | 约 55,149 snapshot validations/s | 无既有目标；建立基线 | 已建档；含线程与锁开销 |

推理分类缓存只绑定单个不可变 `Reasoner` snapshot；本体版本变化会新建 reasoner，因此不跨版本复用。

Knowledge ABox 并发数字是短 burst 基线：每轮启动并 join 8 个 OS 线程，共享进程内
data-consistency cache 由 mutex 序列化缓存访问。它不是纯推理延迟，也不能替代长时 soak、
内存增长或恢复测试。

### HTTP/WS 并发基线（S4b）

负载为真实 release 二进制中的 `fabric submit → approval approve`，审批后执行最小
shell 命令；同时保持 3 个 WebSocket 订阅。10/10 请求成功、0 失败，6 类 fabric
事件各收到 30 次。结果如下：

| 指标 | 数值 |
|------|------|
| Wall time | 0.035347 s |
| Throughput | 282.911 submit/approve per second |
| P50 latency | 16.651 ms |
| P95 latency | 17.986 ms |
| P99 latency | 17.986 ms |

该延迟覆盖 HTTP 提交、治理挂起、人工式 API 审批、子进程调度与终局返回；不是纯
数据库写入耗时。

World query 的平均耗时是返回 500 行的完整批次耗时，不是单行读耗时。
Criterion 报告的是均值和中位数置信区间，不产生 HTTP P99；端到端 API/WS P99 由
S4b 负载测试补齐。当前数字只代表本机开发环境基线，不能直接外推为生产 SLA。

## 复现方式

```bash
make bench
```

```bash
make load-baseline
```

Criterion HTML 和机器可读结果位于 `target/criterion/report/index.html` 与各基准的
`estimates.json`；HTTP/WS 结果写入 `target/performance-load.json`。比较趋势时必须保持
同一硬件、电源、编译 profile 和上述固定负载。

## SLA 语义和边界（S4c）

以上都是本机开发环境证据，不是生产合同 SLA。当前运行时控制面边界是按 principal
独立计数的 token bucket，且限流器故障时 fail-closed：

| 通道 | 突发容量 / 补充周期 | 额外边界 |
|------|--------------------|----------|
| Execution submit | 10 / 60 s | 每 principal |
| Approval resolve | 30 / 60 s | 每 principal |
| WebSocket connect/message | 20 / 60 s | 每 principal，最多 4 个并发连接 |

一致性语义如下：成功返回前，world 变更、审计事件和哈希链节点已在同一个 libSQL
事务中提交，后续查询具有 read-your-writes。WS 实时推送经过容量 1024 的 broadcast
通道，是 best-effort；慢消费者或风暴下可能漏过实时帧。权威数据始终在 append-only
审计日志中，断线重连用 `after_id` 从存储补齐，投影增量用水位游标推进。因此实时
通道的缺口不造成持久化事实丢失，最终一致窗口由重连补齐时间决定，本地嵌入式形态
通常可忽略，跨网络部署必须实测。生产 SLA 需在目标硬件、部署拓扑、备份策略和长期
负载下重新测量后另行声明。
