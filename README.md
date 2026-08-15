# OpenAIP-OS

Ontology-Centric · Local First · Agent Native 的可执行本体平台内核。

用 `.odl` 描述世界对象与动作，经确定性编译进入 Runtime Kernel；Agent（Pi 作 Cognitive Engine）在可观察的世界模型中决策，人可理解、可介入、系统可学习。

## 快速开始

### 依赖

- Rust **nightly**（见 `rust-toolchain.toml`，edition 2024）
- Node.js 18+（仅前端）
- 可选：`pi` 认知引擎（`@earendil-works/pi-coding-agent`），用于真实 LLM NLU；测试使用 FakeEngine

```bash
# 工具链
rustup show   # 应解析到 nightly-2026-07-17

# 质量门禁
make check    # fmt + clippy + test
make ci       # check + ui build + npm audit
```

### 编译本体

```bash
cargo build --release
./target/release/oaip compile ontology.odl
# → ./gen/ontology.rs, schema.sql, ontology.json
```

### 演示数据与 Agent

```bash
# 写入示例 Customer
./target/release/oaip run

# 带声明式动作跑 Agent（默认 ODL=examples/crm.odl）
./target/release/oaip agent run "reduce churn" --auto-approve

# Worker ABI（Phase 11D）：Thinker 制定计划，Worker 执行——OpenAIP-OS 不亲自干活
# 预置 echo/git/codex/claude/deepseek Worker；shell.execute/code.write 需审批（fail-closed）
./target/release/oaip worker run text.echo,code.test --input '{"suite":"unit"}' --task fix-bug

# 启动观察服务（UI + REST + WS）
export WB_BOOTSTRAP_ADMIN_USER=admin
export WB_BOOTSTRAP_ADMIN_PWD='change-me'
./target/release/oaip serve --odl examples/crm.odl --auto-approve
# UI → http://127.0.0.1:8080/
```

首次部署也可用：

```bash
./target/release/oaip user create-admin admin --password 'change-me'
```

## 架构（分层）

| 层 | 模块 | 说明 |
|----|------|------|
| 语言 | `odl` / `ir` / `codegen` | `.odl` → IR → Rust / SQL / JSON |
| 世界 | `storage` / `runtime` | Object / Relation / Event + Trace |
| 决策 | `planner` / `action` / `agent` | Goal→Plan→Policy→Action |
| 学习 | `evolution` | Experience→Pattern→LearnedKnowledge（Learn≠Permission） |
| 观察 | `trace` / `server` / `ui` | 三层视图 + WebSocket 流 |
| 安全 | `auth` | 本地账号 + 会话令牌 + 角色视图钳制 |

演进路线见 `docs/09_迭代与版本/sprint-plan.md`（V0.1→V0.6 已落地）。

## 开发约定

- 原则见根目录 `AGENTS.md`：不保留向后兼容、选最简单实现、分层构建、模块化。
- 文档体系在 `docs/`，约束见 `docs/01_项目总览/project-constraint.md`。
- 代码导航优先用 `graft/`（`graft ask` / `graft build`）。

## 测试

```bash
cargo test --all-targets
```

集成契约覆盖 ODL/IR、存储、关系、Agent 闭环、Action/Policy、Memory/Evolution、认证、持久化恢复、WS 视图钳制。

## 许可证

私有仓库默认保留所有权利；开源许可待定。
