CARGO ?= cargo

.PHONY: fmt lint test build run-player status

fmt:
	$(CARGO) fmt --all

lint:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

test:
	$(CARGO) test --workspace

build:
	$(CARGO) build --workspace

run-player:
	$(CARGO) run -p mba-player

status:
	$(CARGO) run -p mba-cli -- status

