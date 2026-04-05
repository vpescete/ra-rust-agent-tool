.PHONY: build test clippy fmt install clean

build:
	cargo build --release

test:
	cargo test --workspace

clippy:
	cargo clippy --workspace -- -D warnings

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

install:
	cargo install --path crates/ra-cli

clean:
	cargo clean

check: fmt-check clippy test
	@echo "All checks passed!"

# Quick development build
dev:
	cargo build

# Run a single agent
run-agent:
	cargo run -p ra-cli -- agent "$(PROMPT)"

# Run a workflow
run-workflow:
	cargo run -p ra-cli -- run $(WORKFLOW)

# Show history
history:
	cargo run -p ra-cli -- history
