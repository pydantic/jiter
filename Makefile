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

.PHONY: python-install
python-install:
	pip install -U maturin ujson orjson
	pip install -r crates/jiter-python/tests/requirements.txt

.PHONY: python-dev
python-dev:
	maturin develop -m crates/jiter-python/Cargo.toml

.PHONY: python-test
python-test: python-dev
	pytest crates/jiter-python/tests

.PHONY: python-dev-release
python-dev-release:
	maturin develop -m crates/jiter-python/Cargo.toml --release

.PHONY: python-bench
python-bench: python-dev-release
	python crates/jiter-python/bench.py

.PHONY: bench
bench:
	cargo bench -p jiter -F python

.PHONY: fuzz
fuzz:
	cargo +nightly fuzz run --fuzz-dir crates/fuzz compare_to_serde --release

.PHONY: fuzz-skip
fuzz-skip:
	cargo +nightly fuzz run --fuzz-dir crates/fuzz compare_skip --release

.PHONY: all
all: format lint test test-python
