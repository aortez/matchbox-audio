CARGO ?= cargo

.PHONY: fmt lint test build run-player status yocto-build yocto-flash yocto-smoke

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

yocto-build:
	cd yocto && npm run build

yocto-flash:
	cd yocto && npm run flash

yocto-smoke:
	cd yocto && npm run smoke
