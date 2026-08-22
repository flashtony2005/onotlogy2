# 本地与 CI 共用的质量门禁（与 docs/09_迭代与版本/design-ci.md 对齐）。
# 用法：make check（fmt/clippy/test）| make ci（check + ui + audit）
# 需 nightly（见 rust-toolchain.toml）；若 PATH 无 cargo，用 rustup 工具链 bin。

CARGO ?= cargo
export PATH := $(HOME)/.rustup/toolchains/nightly-2026-07-17-aarch64-apple-darwin/bin:$(HOME)/.cargo/bin:$(PATH)

fmt:
	$(CARGO) fmt

fmt-check:
	$(CARGO) fmt --check

clippy:
	$(CARGO) clippy --all-targets -- -D warnings

test:
	$(CARGO) test --workspace --all-targets

# Phase 1：真实 dsh/process + 审批 + WS + 重启恢复黄金场景。
golden:
	$(CARGO) build --bin oaip
	./scripts/golden-scenario.sh

# Phase 3：release binary + real HTTP probes + SIGTERM graceful shutdown.
ops-smoke:
	./scripts/phase3-smoke.sh

# Phase 3：Dockerfile + real container + health/ready/metrics + stop。
container-smoke:
	./scripts/container-smoke.sh

# Phase 3：真实 Compose 文件 + volume + healthcheck + stop。
compose-smoke:
	./scripts/compose-smoke.sh

# Phase 3：真实旧/新二进制 + schema 升级 + 快照恢复 + 二进制回滚演练。
# 必须显式提供 OAIP_BASE_BIN 或 OAIP_BASE_REF，避免候选二进制自测冒充回滚。
ops-rehearsal:
	./scripts/phase3-upgrade-rollback.sh

# Phase 3：生成 binary / schema / Git / container 的发布证据清单。
release-manifest:
	./scripts/release-manifest.sh

ui:
	cd ui && npm ci && npm run build

# macOS：本地 libSQL/SQLite 数据库 + 浏览器 UI 的可分发 .app，不启用云端同步。
mac-app:
	./scripts/build-mac-app.sh

mac-open:
	open target/mac/Ontology.app

audit:
	cd ui && npm audit --audit-level=high

# D7：固定数据规模的四条核心路径基准（结果入 docs/03_架构设计/performance-baseline.md）。
bench:
	$(CARGO) bench --bench performance

# D7/S4b：真实二进制并发 submit/approve + WebSocket 流基线。
load-baseline:
	$(CARGO) build --release --bin oaip
	./scripts/performance-load.py --binary target/release/oaip --odl examples/crm.odl \
		--requests 10 --concurrency 5 --ws-clients 3 --output target/performance-load.json

# CI 主门禁
check: fmt-check clippy test unwrap-budget

# M6：unwrap 预算门禁（scripts/unwrap-budget.txt，只降不升）
unwrap-budget:
	./scripts/unwrap-budget.sh

# 全量（含前端构建与依赖审计）
ci: check ui audit
