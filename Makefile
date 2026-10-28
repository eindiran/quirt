.DEFAULT_GOAL := help

.PHONY: help
help:
	@printf 'Usage: make <target>\n'
	@printf '\n'
	@printf '  help          Show this help\n'
	@printf '  all           Build all targets\n'
	@printf '  test          Run the test suite\n'
	@printf '  lint          Run clippy and cargo audit (read-only)\n'
	@printf '  deny          Check licenses, bans, sources, advisories\n'
	@printf '  format        Apply cargo fmt in place\n'
	@printf '  format-check  Check formatting without changing files\n'
	@printf '  check         format-check, lint, deny, test, and pre-commit\n'
	@printf '  clean         Remove build artifacts\n'

.PHONY: all
all:
	cargo build --all-targets --all-features

.PHONY: test
test:
	cargo test --all-features

.PHONY: lint
lint:
	cargo clippy --all-targets --all-features -- -D warnings
	cargo audit

.PHONY: deny
deny:
	cargo deny check

.PHONY: format
format:
	cargo fmt

.PHONY: format-check
format-check:
	cargo fmt --check

.PHONY: check
check: format-check lint deny test
	pre-commit run --all-files

.PHONY: clean
clean:
	cargo clean
