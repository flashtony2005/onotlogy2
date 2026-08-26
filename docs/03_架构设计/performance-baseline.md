# 核心路径性能基线

## 基本信息

- 记录日期：2026-08-24
- 版本：V0.14.0 候选
- 场景：Local-First 单进程，嵌入式 libSQL `:memory:`
- 微基准方法：`cargo bench --bench performance`（Criterion；`core_paths` 30 个样本，
  `knowledge_abox`、`knowledge_durability` 和 `knowledge_index_scaling` 10 个样本；
  默认 3 秒预热 / 5 秒测量，index scaling 保留 500 ms 快速预热）
- 环境：Apple M4；10 CPU；16 GiB 内存；macOS 27.0.0；arm64
- 基线更新：2026-08-24 新增 sustained concurrency、cache eviction recovery、persisted
  proposal reopen、reasoner consistency latency、index scaling 与 RSS sampling；其余数字记录于
  2026-08-22。

## 固定负载

| 基准 | 负载 |
|------|------|
| ODL compile | 24 个对象类型，每个对象 1 个 action |
| Projection replay | 1,000 条确定性 world event |
| World query | 500 个持久化对象 |
| Audit write | 每次迭代写 1 条事件 + 1 个哈希链节点 |
| Reasoner classification | 24 个 named class 的固定继承链；对比新建 Reasoner 和同一不可变快照重复查询 |
| Reasoner consistency | 同一 24-class 固定链路；对比新建 immutable snapshot 和同 snapshot cached result |
| Knowledge ABox consistency | 1,000 条 functional data property 断言；对比全量重建、exact-result 缓存和固定 append-only delta chain |
| Knowledge ABox concurrency | 同一 warmed 1,000-fact snapshot 上，8 线程 × 128 次 cached validation；包含线程创建/join |
| Sustained ABox concurrency | 上述 burst 连续执行 4 轮，共 4,096 次验证 |
| Cache eviction recovery | 先写入 40 个唯一 snapshot 驱逐目标，再重建 1,000-fact 索引 |
| Persisted knowledge recovery | 重开 file-backed DB 并读取 1 个已提交版本 + 1 个 pending proposal |
| Index scaling | 250/500/1,000/2,000 条 functional assertion 的 cold rebuild |
| HTTP/WS load | 10 次并发 submit/approve（并发 5），3 个 WebSocket 客户端，真实 release 二进制 |
| Reasoner scale | 100/500/1,000/5,000 个 runtime object fact 的 whole-world consistency |
| Knowledge soak / crash drill | 可配置持续时间（默认 1 小时）的 reason/diagnosis 并发读、每 5 秒受控演化、RSS 采样，以及 Pending proposal SIGKILL 重启校验 |

## 基线结果

| 关键路径 | 平均耗时 | 中位数 | 吞吐 | 目标 | 结论 |
|----------|----------|--------|------|------|------|
| ODL parse + codegen | 48.300 µs | 47.905 µs | 约 496,900 objects/s | 单文件 < 200 ms | 达标 |
| Projection replay | 335.503 µs | 335.756 µs | 约 2,980,600 events/s | 无既有目标；建立基线 | 已建档 |
| World object query | 247.964 µs / 批次 | 246.653 µs / 批次 | 约 2,016,400 objects/s | 本地 CRUD < 10 ms | 达标 |
| Audit append + hash chain | 27.961 µs | 27.985 µs | 约 35,764 writes/s | 无既有目标；建立基线 | 已建档 |
| Reasoner classification (new) | 1.358 ms | 约 1.358 ms | 约 17,678 classes/s | 无既有目标；建立基线 | 已建档 |
| Reasoner classification (snapshot cache) | 9.401 µs | 约 9.401 µs | 约 2,553,100 classes/s | 无既有目标；建立基线 | 已建档 |
| Reasoner consistency (new) | 159.80 µs | 约 159.80 µs | 约 150,190 classes/s | 无既有目标；建立基线 | 已建档 |
| Reasoner consistency (snapshot cache) | 1.300 ns | 约 1.300 ns | 约 18,466 Melem/s | 无既有目标；建立基线 | `OnceLock` 命中，接近计时下限 |
| Knowledge ABox consistency (cold rebuild) | 972.585 µs | 964.734 µs | 约 1,028,200 assertions/s | 无既有目标；建立基线 | 已建档 |
| Knowledge ABox consistency (exact-result cache) | 34.745 µs | 34.518 µs | 约 28,780,000 assertions/s | 无既有目标；建立基线 | 已建档 |
| Knowledge ABox consistency (append-only delta chain) | 191.292 µs | 189.999 µs | 约 5,228 appends/s | 无既有目标；建立基线 | 约为全量耗时 19.7% |
| Knowledge ABox concurrent cached validation (8×128) | 18.568 ms / burst | 约 18.568 ms / burst | 约 55,149 snapshot validations/s | 无既有目标；建立基线 | 已建档；含线程与锁开销 |
| Knowledge ABox sustained validation (8×128×4) | 57.831 ms | 约 57.831 ms | 约 70,827 validations/s | 无既有目标；建立基线 | 4 轮无迟滞放大 |
| Knowledge ABox eviction recovery (1,000 facts) | 1.215 ms | 约 1.215 ms | 约 823,000 assertions/s | 无既有目标；建立基线 | 完整重建恢复 |
| Persisted pending proposal reopen | 1.653 ms | 约 1.653 ms | 约 1,210 records/s | 无既有目标；建立基线 | 含 SQLite 打开/迁移与版本/proposal 读取 |
| Index cold rebuild @250/@500/@1,000/@2,000 | 250.8 µs / 528.3 µs / 1.133 ms / 2.436 ms | — | 997K / 946K / 883K / 821K assertions/s | 无既有目标；建立基线 | 近线性扩展，大 ABox 吞吐缓降 |

推理分类缓存只绑定单个不可变 `Reasoner` snapshot；本体版本变化会新建 reasoner，因此不跨版本复用。

Knowledge ABox 并发数字包含每轮启动并 join 8 个 OS 线程的开销；共享进程内 data-consistency
cache 由 mutex 序列化缓存访问。sustained baseline 覆盖连续 4 轮的稳态行为，但仍不是生产 soak。
Reasoner consistency new 包含 OWL conversion、data fragment 检查和 mature tableau 调用；cached
result 只测量 immutable snapshot 上的 `OnceLock` 读取。eviction recovery 测量 bounded LRU 驱逐后
的完整索引重建。persisted reopen 是真实 file-backed storage 的版本和 pending proposal 恢复延迟。

HTTP/WS 负载脚本现在按固定间隔采样 server RSS。默认基线记录到 first≈32 KiB（fork 后立即采样）、
peak/final≈78.9 MiB、4 个样本；first 值反映 exec 前窗口，容量评估应以 peak/final 为准。
该采样是开发环境冒烟证据，不是长期内存增长证明。

### HTTP/WS 并发基线（S4b）

负载为真实 release 二进制中的 `fabric submit → approval approve`，审批后执行最小
shell 命令；同时保持 3 个 WebSocket 订阅。10/10 请求成功、0 失败，6 类 fabric
事件各收到 30 次。2026-08-24 结果如下：

| 指标 | 数值 |
|------|------|
| Wall time | 0.050886 s |
| Throughput | 196.518 submit/approve per second |
| P50 latency | 24.121 ms |
| P95 latency | 25.639 ms |
| P99 latency | 25.639 ms |
| RSS first / peak / final | 32 KiB / 80,816 KiB / 80,816 KiB（4 个样本） |

该延迟覆盖 HTTP 提交、治理挂起、人工式 API 审批、子进程调度与终局返回；不是纯
数据库写入耗时。

World query 的平均耗时是返回 500 行的完整批次耗时，不是单行读耗时。
Criterion 报告的是均值和中位数置信区间，不产生 HTTP P99；端到端 API/WS P99 由
S4b 负载测试补齐。当前数字只代表本机开发环境基线，不能直接外推为生产 SLA。

### Reasoner 规模基线（2026-08-26）

`runtime_world_to_knowledge()` 使用二进制身份分区表达所有 storage ID 的两两互异：
每个对象进入 `ceil(log2(N))` 个 0/1 partition class，每对 partition class 声明不相交。
任意两个不同编码的对象至少在一个位上冲突，因此语义仍严格等价于全局 pairwise distinct，
但避免上游把 N 元 `DifferentIndividuals` 展开成 `N*(N-1)/2` 对。该路径不挑选“相关个体”，
也不削弱 functional、inverse-functional、cardinality 或 nominal 身份推理。

Apple M4 release 模式、`benches/reasoner-scale.rs` 最新结果如下；耗时包含事实生成、
world projection、OWL conversion 和 `Reasoner::new() + is_consistent()`：

| 对象数 | KB 公理 | 转换耗时 | 推理一致性耗时 |
|-------:|-------:|-------:|---------------:|
| 100 | 909 | 0.8 ms | 6.1 ms |
| 500 | 5,511 | 1.2 ms | 23.4 ms |
| 1,000 | 12,012 | 1.4 ms | 33.4 ms |
| 5,000 | 75,015 | 6.1 ms | 211.1 ms |

修复前同一 5k 负载约需 80.65 秒。CI 通过 `make reasoner-scale-sla` 固定
`5,000 objects <= 1,000 ms` 门禁；这是本机回归红线，仍不等同于跨硬件生产合同 SLA。

## 复现方式

```bash
make bench
```

```bash
make reasoner-scale-sla
```

```bash
make load-baseline
```

目标硬件上的知识 soak 与灾难恢复演练：

```bash
make soak-baseline SOAK_DURATION=3600 SOAK_CONCURRENCY=4 \
  SOAK_MUTATION_INTERVAL=5 SOAK_MEMORY_WARMUP=60
```

Criterion HTML 和机器可读结果位于 `target/criterion/report/index.html` 与各基准的
`estimates.json`；HTTP/WS 结果写入 `target/performance-load.json`；知识 soak 与 SIGKILL 恢复
结果写入 `target/knowledge-soak.json`，其中包含 Git revision、二进制/ODL SHA-256、主机与 CPU
信息。比较趋势时必须保持同一硬件、电源、编译 profile 和上述固定负载。当前仓库尚未归档目标
硬件的 1 小时 soak 产物；上述命令是复现入口，不是已完成的性能结论。

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
