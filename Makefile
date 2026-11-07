.PHONY: build test fmt fmt-check clippy lint check-licenses check-targets check-docs buf-lint ci

CARGO ?= cargo
BUF ?= buf
MOBILE_TARGETS := aarch64-apple-ios aarch64-linux-android
# Crates that must compile for mobile clients. Grows as client crates are added.
MOBILE_CRATES := -p urushi-core -p urushi-proto -p urushi-client

build:
	$(CARGO) build --workspace --all-targets

test:
	$(CARGO) test --workspace --all-targets
	$(CARGO) test --workspace --doc

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all -- --check

clippy:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

lint: fmt-check clippy

check-licenses:
	$(CARGO) deny check

check-targets:
	@for t in $(MOBILE_TARGETS); do \
		echo "==> $$t"; \
		$(CARGO) check $(MOBILE_CRATES) --target $$t || exit 1; \
	done

check-docs:
	./scripts/check-docs.sh

buf-lint:
	@if [ -n "$$(ls proto/urushi/v1/*.proto 2>/dev/null)" ]; then $(BUF) lint; else echo "no protos yet"; fi

ci: lint test check-licenses check-docs buf-lint
