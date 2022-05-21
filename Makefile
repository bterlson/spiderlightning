.PHONY: build
build:
	cargo build --release
	cargo build --target wasm32-wasi --release --manifest-path ./examples/kv-filesystem-config/Cargo.toml
	cargo build --target wasm32-wasi --release --manifest-path ./examples/kv-azure-blob-config/Cargo.toml
	cargo build --target wasm32-wasi --release --manifest-path ./examples/kv-demo/Cargo.toml

.PHONY: test
test:
	cargo test --all --no-fail-fast -- --nocapture
	cargo clippy --all-targets --all-features -- -D warnings
	cargo fmt --all -- --check

.PHONY: run
run:
	./target/release/wasi-cloud -m ./target/wasm32-wasi/release/kv-demo.wasm -c ./target/wasm32-wasi/release/kv_filesystem_config.wasm
	./target/release/wasi-cloud -m ./target/wasm32-wasi/release/kv-demo.wasm -c ./target/wasm32-wasi/release/kv_azure_blob_config.wasm