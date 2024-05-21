.DEFAULT_GOAL := all

.PHONY: format
format:
	@cargo fmt --version
	cargo fmt

.PHONY: lint
lint:
	@cargo clippy --version
	cargo clippy -- -D warnings
	cargo doc

.PHONY: test
test:
	cargo test

.PHONY: python-dev
python-dev:
	maturin develop -m crates/jiter-python/Cargo.toml

.PHONY: test-python
test-python: python-dev
	pytest crates/jiter-python/tests

.PHONY: bench
bench:
	cargo bench  -p jiter -F python

.PHONY: fuzz
fuzz:
	cargo +nightly fuzz run --fuzz-dir crates/fuzz compare_to_serde --release

.PHONY: fuzz-skip
fuzz-skip:
	cargo +nightly fuzz run --fuzz-dir crates/fuzz compare_skip --release

.PHONY: all
all: format lint test test-python
