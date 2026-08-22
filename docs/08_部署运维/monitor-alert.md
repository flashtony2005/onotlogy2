# 监控告警

## 1. 探针与指标

应用提供免鉴权运维端点：

| 指标/端点 | 语义 | 建议告警 |
|---|---|---|
| `/healthz` | 进程仍在响应 | 连续 2 次失败：实例故障 |
| `/readyz` | schema 与审计链可用 | 任意 1 次 503：摘除流量并调查 |
| `oaip_ready` | readiness 数值化 | `== 0` 持续 1 分钟：高 |
| `oaip_pending_approvals` | 当前挂起审批数量 | 按业务容量设置，持续增长需调查 |
| `oaip_storage_errors` | 指标采集期间存储读取失败次数 | `> 0`：高 |

`/metrics` 使用 Prometheus text exposition 格式，当前是低依赖的派生快照；部署侧应按
抓取间隔记录时间序列，不将其误认为累计 counter。

## 2. 最小抓取配置

```yaml
scrape_configs:
  - job_name: openaip
    static_configs:
      - targets: ['127.0.0.1:8080']
    metrics_path: /metrics
```

生产环境必须通过内网或反向代理暴露这些端点，不要将未经 TLS/访问控制的 metrics
端口直接暴露到公网。
