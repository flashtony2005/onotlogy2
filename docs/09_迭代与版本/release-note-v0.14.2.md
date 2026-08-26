# OpenAIP-OS V0.14.2 发布说明：可执行本体生产核心 GA

| 项 | 内容 |
|----|------|
| 版本 | V0.14.2 |
| 发布日期 | 2026-08-26 |
| Schema | 20（与 `v0.14.x` 兼容） |
| 发布状态 | **生产核心 GA** |
| 证据锚点 | 干净工作树 + `v0.14.2` tag + `target/release-manifest.json` |

## 一句话结论

OpenAIP-OS 的可执行本体内核已从“测试通过的原型”收敛为可部署的生产核心：企业状态用 ODL 与事件溯源世界表达，
推理器校验一致性，Agent 在治理边界内发现机会、选择动作、执行状态变化，并根据结果受控演化；
数据、审批、审计、备份、恢复和容器部署均有自动化证据。

## 本次正式成立的能力闭环

### 1. 可执行本体

- `.odl` 编译为 IR 和 RuntimeDefinition，再进入 Kernel / Capability / Policy / Task 执行面。
- 世界状态来自 Event Log 投影，投影不反向写入事件日志；对象、关系、标量和约束进入统一知识视图。
- 本体演进有兼容门禁；行为变更显式化，未知动作 fail-closed。

### 2. 企业状态理解与商业行动

- Business Loop 覆盖机会发现、目标度量、动作选择、预算/资源检查、执行、失败补偿、重开机会和再规划。
- 动作必须有声明式 actor、参数、前置条件、收益指标和可验证 follow-up；没有证据的收益不能作为学习结论。
- 推理一致性门禁阻止不一致世界进入业务执行。

### 3. 知识推理与规模红线

- 身份投影使用 schema 指纹隔离命名空间，保留 pairwise distinctness 语义，消除固定合成类冲突。
- Apple M4 本机基线中，5,000 个对象的 whole-world consistency 推理阶段约 211–233ms，
  本机 SLA 红线为 1000ms；GitHub 共享 runner 使用 3000ms 预算。该数字不是跨硬件生产合同。

### 4. Agent 执行与治理

- Agent Fabric 支持异步提交、实时观察、取消、超时、崩溃恢复和进程组回收。
- 高风险动作进入人工审批；学习只能提出建议或调整统计，不能授予能力、审批或绕过策略。
- 所有世界变化进入审计事件；审计链 append-only、可导出、可校验。

### 5. 部署与运维

- 单机 Local-First 形态提供 release binary、Docker image、Compose、health/ready/metrics、优雅停机、
  备份、恢复、schema 迁移、升级回滚演练和发布 manifest。
- Compose 冒烟验证 named volume 中真实世界事件跨 `stop → start` 存活。

## DoD 全量复核

详细逐项证据见 [dod-review-v0.14.2.md](dod-review-v0.14.2.md)。结论：

| 结论 | 说明 |
|------|------|
| D1–D12 | ✅ 工程验收全部关闭 |
| Schema | ✅ 20；up/down、失败回滚和跨 binary 演练已覆盖 |
| 测试 | ✅ workspace、UI、coverage、golden、container/compose smoke 通过 |
| 性能 | ✅ 固定负载基线和 Reasoner 规模红线入档 |
| RPO | ⚠️ 产品提供 15 分钟备份语义；目标环境仍需接入外部调度器并留存成功/告警证据 |

## 升级路径

从 `v0.14.0` 或 `v0.14.1` 升级到 `v0.14.2` 时 schema 仍为 20：

1. 停止服务并确认 `/readyz` 已不再接收流量。
2. 执行一次 `oaip backup` 并验证备份可读。
3. 替换 release binary 或更新到 `openaip-os:0.14.2` 镜像。
4. 启动后检查 `/healthz`、`/readyz`、`/metrics` 和审计链健康。

同 schema 补丁路径已用真实旧/新 binary 演练。若必须回滚二进制，先恢复或保留升级前快照，
并按 rollback plan 记录实际 RPO/RTO。

## 明确不宣称的边界

- 本版本不宣称多区域高可用、分布式共识或托管服务 SLA。
- 性能数字是本机开发环境基线；生产合同必须在目标硬件和拓扑复测。
- RPO ≤15 分钟依赖运营环境的外部定时备份、失败告警和真实写入窗口演练。
- 外部 LLM/DSH/Codex/Pi 是可选 Worker；核心 golden 场景使用确定性 process executor，不把外部凭证当成产品依赖。

---

关联文档：[version-changelog.md](version-changelog.md)、[deploy-steps.md](../08_部署运维/deploy-steps.md)、
[rollback-plan.md](../08_部署运维/rollback-plan.md)、[backup-restore.md](../08_部署运维/backup-restore.md)、
[performance-baseline.md](../03_架构设计/performance-baseline.md)。
