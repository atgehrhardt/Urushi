.PHONY: icons app install dev ship ios-sim ios android build test fmt fmt-check clippy lint check-licenses check-targets check-docs buf-lint ci

CARGO ?= cargo
BUF ?= buf
# Crates that must compile for mobile clients. Grows as client crates are added.
MOBILE_CRATES := -p urushi-core -p urushi-proto -p urushi-client -p urushi-ui -p urushi-render

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
	$(CARGO) check $(MOBILE_CRATES) -p urushi-ios --target aarch64-apple-ios
	$(CARGO) check $(MOBILE_CRATES) -p urushi-android --target aarch64-linux-android

# Re-render every platform's app icon from assets/icon/icon.svg (macOS only; outputs are committed).
icons:
	swift scripts/render-icons.swift

app:
	./scripts/bundle-macos.sh

# Install into /Applications without touching the running app or daemon.
install:
	./scripts/install-macos.sh

# Run the debug GUI against the dev daemon (never your real workspace). See scripts/dev.sh.
dev:
	./scripts/dev.sh run

# Test, then install into /Applications without touching your running daemon.
ship:
	./scripts/dev.sh ship

ios-sim:
	./scripts/bundle-ios.sh --run

# Unsigned device build: build/ios/Urushi.ipa (re-sign it to install; see docs/platforms.md).
ios:
	./scripts/bundle-ios.sh --device

android:
	./scripts/build-android.sh

check-docs:
	./scripts/check-docs.sh

buf-lint:
	@if [ -n "$$(ls proto/urushi/v1/*.proto 2>/dev/null)" ]; then $(BUF) lint; else echo "no protos yet"; fi

ci: lint test check-licenses check-docs buf-lint
