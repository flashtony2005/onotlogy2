# 覆盖率基线（Coverage Baseline）

| 项 | 值 |
|----|----|
| 采集日期 | 2026-08-14 |
| 工具 | cargo-llvm-cov v0.8.7（nightly-2026-07-17 + llvm-tools-preview） |
| 范围 | `cargo llvm-cov --all-targets`（lib 单测 + 全部集成测试，118 用例） |
| **行覆盖** | **75.73%**（7749 行 / 已覆盖 5868） |
| 分支覆盖 | 78.43% |
| 函数覆盖 | 79.93% |
| lib 单测单独基线 | 19.41%（集成测试承载了主要覆盖） |

## 最低覆盖模块（提升优先级）

| 模块 | 行覆盖 | 说明 |
|------|--------|------|
| `server/static_files.rs` | 0% | 静态托管/SPA fallback 无任何断言（serve_static/content_type） |
| `tool_backend.rs` | 25% | NullBackend 直通；真实 backend 需注入实现测试 |
| `ir.rs` | 50% | IR 结构/序列化边界（部分经 codegen 间接覆盖） |
| `planner/mod.rs` | 82% | 核心类型少量直接断言 |
| `server/graph.rs` | 72% | /graph 端点（经 auth 测试覆盖主路径） |
| `codegen.rs` | 75% | 生成的 Rust 代码未编译验证（compile 产物仅文本断言） |

## 使用方式

```bash
# 生成覆盖率报告（HTML）
cargo llvm-cov --all-targets --open

# 生成 LCOV（CI 集成用）
cargo llvm-cov --all-targets --lcov --output-path lcov.info

# 对比基线：覆盖率回退时提示
cargo llvm-cov --all-targets --fail-under-lines 75
```

## 基线用途

1. 新增功能/重构后跑一次，覆盖不应明显回退（`--fail-under-lines 75` 可作 CI 门槛）
2. 优先补齐上表低覆盖模块（static_files 零覆盖是第一个目标）
3. 覆盖率是**代理指标**：118 用例的失败路径/并发/安全断言仍是主线，覆盖率用于发现"完全没测到"的区域
