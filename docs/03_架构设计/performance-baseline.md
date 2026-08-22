# 核心路径性能基线

## 基本信息

- 记录日期：2026-08-22
- 版本：V0.14.0 候选
- 场景：Local-First 单进程，嵌入式 libSQL `:memory:`
- 方法：`cargo bench --bench performance`（Criterion，30 个样本，默认 3 秒预热 / 5 秒测量）
- 环境：Apple M4；10 CPU；16 GiB 内存；macOS 27.0.0；arm64

## 固定负载

| 基准 | 负载 |
|------|------|
| ODL compile | 24 个对象类型，每个对象 1 个 action |
| Projection replay | 1,000 条确定性 world event |
| World query | 500 个持久化对象 |
| Audit write | 每次迭代写 1 条事件 + 1 个哈希链节点 |

## 基线结果

| 关键路径 | 平均耗时 | 中位数 | 吞吐 | 目标 | 结论 |
|----------|----------|--------|------|------|------|
| ODL parse + codegen | 48.300 µs | 47.905 µs | 约 496,900 objects/s | 单文件 < 200 ms | 达标 |
| Projection replay | 335.503 µs | 335.756 µs | 约 2,980,600 events/s | 无既有目标；建立基线 | 已建档 |
| World object query | 247.964 µs / 批次 | 246.653 µs / 批次 | 约 2,016,400 objects/s | 本地 CRUD < 10 ms | 达标 |
| Audit append + hash chain | 27.961 µs | 27.985 µs | 约 35,764 writes/s | 无既有目标；建立基线 | 已建档 |

World query 的平均耗时是返回 500 行的完整批次耗时，不是单行读耗时。
Criterion 报告的是均值和中位数置信区间，不产生 HTTP P99；端到端 API/WS P99 由
S4b 负载测试补齐。当前数字只代表本机开发环境基线，不能直接外推为生产 SLA。

## 复现方式

```bash
make bench
```

HTML 和机器可读结果位于 `target/criterion/report/index.html` 与各基准的
`estimates.json`。比较趋势时必须保持同一硬件、电源、编译 profile 和上述固定负载。
