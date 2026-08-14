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
	$(CARGO) test --all-targets

ui:
	cd ui && npm ci && npm run build

audit:
	cd ui && npm audit --audit-level=high

# CI 主门禁
check: fmt-check clippy test

# 全量（含前端构建与依赖审计）
ci: check ui audit
