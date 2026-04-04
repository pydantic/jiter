.DEFAULT_GOAL := all
python_sources = crates/jiter/benches/generate_big.py crates/jiter-python/bench.py crates/jiter-python/jiter.pyi crates/jiter-python/tests/test_jiter.py


.PHONY: .uv
.uv:
	@uv -V || echo 'Please install uv: https://docs.astral.sh/uv/getting-started/installation/'

.PHONY: format
format:
	@cargo fmt --version
	cargo fmt

.PHONY: lint
lint:
	@cargo clippy --version
	cargo clippy -- -D warnings
	cargo doc

.PHONY: lint-python
lint-python: .uv
	uv run ruff check $(python_sources)
	uv run ruff format --check $(python_sources)

.PHONY: test
test:
	cargo test

.PHONY: python-install
python-install:
	uv sync --all-groups --all-packages

.PHONY: python-dev
python-dev:
	maturin develop -m crates/jiter-python/Cargo.toml

.PHONY: python-test
python-test: python-dev
	uv run pytest crates/jiter-python/tests

.PHONY: python-dev-release
python-dev-release:
	maturin develop -m crates/jiter-python/Cargo.toml --release

.PHONY: python-bench
python-bench: python-dev-release
	uv run python crates/jiter-python/bench.py

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
