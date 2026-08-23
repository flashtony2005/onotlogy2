# OpenAIP-OS

Ontology-Centric · Local First · Agent Native 的可执行本体平台内核（**V2 架构**）。

用 `.odl` 描述世界对象与动作，经确定性编译进入 Runtime Kernel（四 Primitive：
Context / Fiber / Effect / Relation）；Agent 在**一条认知闭环**（Observe → Decide → Act →
Verify）中决策，人可理解、可介入、系统可学习（Learn ≠ Permission）。架构总图见
`docs/03_架构设计/architecture-v2.md`。

## 快速开始

```bash
rustup show                      # nightly-2026-07-17
make check                       # fmt + clippy + test + unwrap 预算

cargo build --release
./target/release/oaip compile ontology.odl        # → ./gen/
./target/release/oaip run                          # 演示数据（Customer 001/002）

# Agent（PiEngine 认知引擎；pi 未装时给出 WB2004 提示）
./target/release/oaip agent run "reduce churn" --auto-approve

# Worker ABI：Thinker 决策 → Worker 执行（echo/git/codex/claude 是 Worker，不是依赖）
./target/release/oaip agent run "fix bug" --via-workers --auto-approve

# 观察服务（UI + REST + WS；V0.7 认证）
export WB_BOOTSTRAP_ADMIN_USER=admin WB_BOOTSTRAP_ADMIN_PWD='change-me'
./target/release/oaip serve --odl examples/crm.odl --auto-approve
# UI → http://127.0.0.1:8080/
```

## 架构（五层）

| 层 | 模块 | 说明 |
|----|------|------|
| L0 | `kernel/` | 四 Primitive + 状态机 + 补偿 + 事件轨（唯一状态源） |
| L1 | `ir/ odl/ codegen/ compose/defs.rs` | `.odl` → IR → RuntimeDefinition（ABI） |
| L2 | `compose/ world_exec/` | Service/Capability/Policy/Task + 世界效应执行 |
| L3 | `cognitive/ evolution2/ rule/ skill/` | 认知闭环 + 一层演化口径 |
| L4 | `worker/ plugin/` | Capability → Router → Worker（Provider Agnostic） |
| L5 | `trace/ server/ ui/ storage/` | 观察层 + 持久化 + 认证 |

设计总纲：`docs/03_架构设计/architecture-v2.md`；知识推理与治理契约：
`docs/03_架构设计/knowledge-reasoning-v1.md`；技术债清零计划与差集：
`docs/09_迭代与版本/tech-debt-reduction-plan.md`。

## 关键概念

- **审批**：Waiting fiber 是唯一待审队列（CLI stdin / server HTTP + 超时看门狗）。
- **审计**：approval.* / evolution.gate_approved 桥进 wb_trace_event → `GET /audit`。
- **模拟**：S1–S3 全是假设轨（I2），永不写 EventLog。
- **不变量**：I1 State=Reduce(EventLog) · I5 Learn≠Permission · I7 EveryRun→Evaluation。

## macOS 本地版

当前提供不依赖 dsh、暂不连接 Turso 云端的本地 `.app`：

```bash
make mac-app
open target/mac/Ontology.app
```

本地数据库默认位于 `~/Library/Application Support/Ontology/ontology.db`，数据库使用 libSQL 的本地文件模式，业务表保持 SQLite 兼容。首次启动生成的管理员凭据位于同目录的 `first-run-credentials.txt`。完整说明见 `docs/08_部署运维/mac-local-app.md`。

## 测试

```bash
cargo test --all-targets    # 326+：V2 契约（tests/v2_parity + *_v2）为核心资产
```

## 许可证

私有仓库默认保留所有权利；开源许可待定。
