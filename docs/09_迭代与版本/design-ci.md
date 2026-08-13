# 技术设计：CI 流水线门禁（V0.7 · 对应 P2 风险「工程化基础设施缺失」）

| 版本 | V0.1（设计稿，待评审） |
| --- | --- |
| 关联风险 | P2：无 CI、fmt/clippy/test 靠人工、无 UI 测试、npm audit 有漏洞 |
| 目标版本 | V0.7 |
| 状态 | 设计稿 · 未动工 |
| 作者 | lushaohui |

---

## 1. 背景与问题

现状核对：无 `.github/workflows`、无 `Makefile`、无 `Dockerfile`、无 `playwright.config.*`、`src/auth*` 不存在。质量门禁（fmt/clippy/test）全靠人工跑；前端 `npm run build` 无 CI 卡点；`npm audit` 历史报过漏洞未自动拦截。

## 2. 目标 / 非目标

**目标**

- PR + main 自动跑质量门禁：Rust（fmt/clippy/test）、UI（build/audit）。
- 提供本地等价入口（`Makefile`），让本地与 CI 一致。
- 加薄 E2E 冒烟（Playwright），锁住「能启动、能登录页、能跑通核心路径」。
- 工具链版本固定（`rust-toolchain.toml`），避免 nightly 漂移（edition 2024）。

**非目标**

- 不搭建发布/部署流水线（部署见 `docs/08_部署运维/`，另议）。
- 不强制 pre-commit hook（以 CI 为准，避免本地摩擦）。

## 3. 设计

### 3.1 工具链固定

新增 `rust-toolchain.toml`：

```toml
[toolchain]
channel = "nightly-2026-08-01"   # 与当前本地 nightly 对齐，edition 2024
components = ["rustfmt", "clippy"]
```

新增 `Makefile`（本地与 CI 共用）：

```makefile
fmt:
	cargo fmt
fmt-check:
	cargo fmt --check
clippy:
	cargo clippy --all-targets -- -D warnings
test:
	cargo test --all-targets
ui:
	cd ui && npm ci && npm run build
audit:
	cd ui && npm audit --audit-level=high
check: fmt-check clippy test      # CI 主门禁
ci: check ui audit                # 全量
```

### 3.2 GitHub Actions：`.github/workflows/ci.yml`

```yaml
name: ci
on:
  push:
    branches: [main]
  pull_request:
concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true

jobs:
  rust:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - run: make fmt-check
      - run: make clippy
      - run: make test

  ui:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with: { node-version: 20, cache: npm, cache-dependency-path: ui/package-lock.json }
      - run: make ui
      - run: make audit

  e2e:                       # 阶段二：薄冒烟
    runs-on: ubuntu-latest
    needs: [rust, ui]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly
      - uses: actions/setup-node@v4
        with: { node-version: 20 }
      - run: make ui
      - run: cd ui && npx playwright install --with-deps chromium
      - run: |
          cargo build -p openaip-os --bin oaip
          ./target/debug/oaip serve --db /tmp/ci.db --static-dir ui/dist --port 18765 &
          SERVER_PID=$!
          cd ui && npx playwright test
          kill $SERVER_PID
```

- `rust` / `ui` 门禁先行；`e2e` 作为第二阶段（依赖前两者，薄冒烟）。
- `cargo` 缓存（`rust-cache`）、`npm` 缓存（setup-node `cache`）加速。

### 3.3 Playwright 薄冒烟（阶段二）

- 新增 `ui/playwright.config.ts` + `ui/tests/e2e/smoke.spec.ts`（`npm i -D @playwright/test`）。
- 冒烟用例：① 启动 `oaip serve` + `ui/dist`；② `GET /` 返回 200 且含登录表单；③ `POST /api/v1/wb/auth/login` 用引导 admin 返回 token；④ `GET /api/v1/wb/agent/trace` 带 token 返回 200。
- 不覆盖全部交互，仅锁「启动 + 鉴权 + 核心读」关键路径，防止回归崩。

### 3.4 npm audit 治理

- 当前漏洞先 `npm audit fix`（非 breaking 范围内），锁版本；CI 以 `--audit-level=high` 卡断。
- moderate/low 暂不阻断（避免频繁红），定期跟进。

### 3.5 测试矩阵（CI 自身即验证）

- `make check` 全绿 = fmt/clippy/test 通过；`make ui` / `make audit` 绿；`e2e` 冒烟通过。
- 任一 job 红 → PR 不可合入（branch protection 规则在仓库侧配置，本设计不覆盖）。

## 4. 设计分叉（已定稿 · 2026-08-13 评审结论）

| # | 分叉 | 结论 | 理由 |
| --- | --- | --- | --- |
| C1 | Playwright E2E 时机 | 阶段二薄冒烟，但**阶段二=Auth 落地后** | 冒烟用例依赖 `/auth/login`，Auth 没做前 E2E 必红 |
| C2 | `npm audit` 失败阈值 | high 阻断 | 符合当前风险等级；moderate 定期处理 |
| C3 | release build job | **加 `cargo build --release`** | 临近产品化，release profile 能抓 debug 抓不到的问题，成本低 |
| C4 | 自托管 runner | 否（ubuntu-latest） | 够用 |
| C5 | pre-commit hook | 否（CI 为准） | 避免本地摩擦 |

**评审补充建议（已吸收进实现）：**

- `rust-toolchain.toml` 固定 `nightly-2026-07-17`（真实本地 nightly，非设计稿误写的 08-01）；ci.yml **不传 `toolchain` 输入** → action 自动读文件，杜绝漂移。
- `npm audit fix` **不加 `--force`**，仅做非 breaking 修复后锁版本，避免冲掉大版本。
- e2e job 的 `--db` 用 `$RUNNER_TEMP`（已写在注释模板），与本地行为一致。

## 5. 修订日志

| 版本 | 作者 | 日期 | 说明 |
| --- | --- | --- | --- |
| V0.2 | lushaohui | 2026-08-13 | 评审定稿：C1–C5 结论落表；补充建议（toolchain 读文件/audit 不加 --force/$RUNNER_TEMP）吸收；rust/ui/release 三 job + Makefile + rust-toolchain.toml 已落地，E2E job 注释预留（Auth 后开） || V0.1 | lushaohui | 2026-08-13 | 初稿：rust-toolchain.toml + Makefile + ci.yml（rust/ui/e2e 三 job）+ Playwright 薄冒烟 + npm audit 治理；列 5 项分叉 |
