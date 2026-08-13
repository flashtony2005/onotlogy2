# 本地与 CI 共用的质量门禁（与 docs/09_迭代与版本/design-ci.md 对齐）。
# 用法：make check（fmt/clippy/test）| make ci（check + ui + audit）

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

# CI 主门禁
check: fmt-check clippy test

# 全量（含前端构建与依赖审计）
ci: check ui audit
