# 本地与 CI 共用的质量门禁（与 docs/09_迭代与版本/design-ci.md 对齐）。
# 用法：make check（fmt/clippy/test）| make ci（check + ui + audit）
# 需 nightly（见 rust-toolchain.toml）；若 PATH 无 cargo，用 rustup 工具链 bin。

CARGO ?= cargo
LOAD_REQUESTS ?= 10
LOAD_CONCURRENCY ?= 5
LOAD_WS_CLIENTS ?= 3
SOAK_DURATION ?= 3600
SOAK_CONCURRENCY ?= 4
SOAK_MUTATION_INTERVAL ?= 5
SOAK_MEMORY_WARMUP ?= 60
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
	cd ui && npm audit --registry=https://registry.npmjs.org/ --audit-level=high

# D7：固定数据规模的四条核心路径基准（结果入 docs/03_架构设计/performance-baseline.md）。
bench:
	$(CARGO) bench --bench performance

# D7：5k world identity projection + whole-world consistency 的回归红线。
reasoner-scale-sla:
	REASONER_SCALE_SLA_MS=1000 $(CARGO) test --release --bench reasoner-scale -- --nocapture

# D7/S4b：真实二进制并发 submit/approve + WebSocket 流基线。
load-baseline:
	$(CARGO) build --release --bin oaip
	./scripts/performance-load.py --binary target/release/oaip --odl examples/crm.odl \
		--requests $(LOAD_REQUESTS) --concurrency $(LOAD_CONCURRENCY) \
		--ws-clients $(LOAD_WS_CLIENTS) --output target/performance-load.json

# S4d：在目标硬件上执行知识推理、受控演化、RSS 增长和 SIGKILL 恢复基线。
# 默认持续 1 小时；发布证据必须保留 target/knowledge-soak.json 和运行环境元数据。
soak-baseline:
	mkdir -p target
	$(CARGO) build --release --bin oaip
	./scripts/knowledge-soak.py --binary target/release/oaip --odl examples/crm.odl \
		--output target/knowledge-soak.json --duration-seconds $(SOAK_DURATION) \
		--concurrency $(SOAK_CONCURRENCY) --mutation-interval $(SOAK_MUTATION_INTERVAL) \
		--memory-warmup-seconds $(SOAK_MEMORY_WARMUP)

# D10：真实二进制备份/恢复演练；64 MiB 是冒烟规模，发布证据应另跑 1 GiB。
BACKUP_TARGET_MIB ?= 64
BACKUP_PAYLOAD_KIB ?= 16
BACKUP_CONCURRENCY ?= 8

OAIP_TARGET_DIR ?= $(HOME)/.cargo/target

backup-rehearsal:
	mkdir -p target
	$(CARGO) build --release --target-dir $(OAIP_TARGET_DIR) --bin oaip
	./scripts/backup-rpo-rto-rehearsal.py \
	  --binary $(OAIP_TARGET_DIR)/release/oaip \
	  --odl examples/crm.odl \
	  --target-mib $(BACKUP_TARGET_MIB) \
	  --payload-kib $(BACKUP_PAYLOAD_KIB) \
	  --concurrency $(BACKUP_CONCURRENCY) \
	  --output target/backup-rehearsal.json

# CI 主门禁
check: fmt-check clippy test unwrap-budget

# M6：unwrap 预算门禁（scripts/unwrap-budget.txt，只降不升）
unwrap-budget:
	./scripts/unwrap-budget.sh

# 全量（含前端构建、依赖审计与推理规模 SLA）
ci: check ui audit reasoner-scale-sla
