.DEFAULT_GOAL := all
python_sources = crates/jiter/benches/generate_big.py crates/jiter/benches/criterion_table.py crates/jiter-python/bench.py crates/jiter-python/jiter.pyi crates/jiter-python/tests/test_jiter.py

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
	# build without any simd arch active
	cargo clippy --target wasm32-wasip1 -p jiter -- -D warnings
	cargo doc

.PHONY: lint-python
lint-python: .uv
	uv run ruff check $(python_sources)
	uv run ruff format --check $(python_sources)

.PHONY: format-python
format-python: .uv
	uv run ruff format $(python_sources)
	uv run ruff check --fix --fix-only $(python_sources)

.PHONY: test
test:
	cargo test

.PHONY: python-install
python-install:
	uv sync --all-groups --all-packages

.PHONY: python-dev
python-dev:
	uv run maturin develop --uv -m crates/jiter-python/Cargo.toml

.PHONY: python-test
python-test: python-dev
	uv run pytest crates/jiter-python/tests

.PHONY: python-dev-release
python-dev-release:
	uv run maturin develop --uv -m crates/jiter-python/Cargo.toml --release

.PHONY: python-bench
python-bench:
	uv sync --group bench
	$(MAKE) python-dev-release
	uv run --no-sync crates/jiter-python/bench.py

.PHONY: bench
bench:
	cargo bench -p jiter -F python

.PHONY: bench-table-display
bench-table-display:
	uv run crates/jiter/benches/criterion_table.py

.PHONY: bench-table
bench-table:
	rm -rf target/criterion
	$(MAKE) bench
	$(MAKE) bench-table-display

.PHONY: fuzz
fuzz:
	cargo +nightly fuzz run --fuzz-dir crates/fuzz compare_to_serde --release

.PHONY: fuzz-skip
fuzz-skip:
	cargo +nightly fuzz run --fuzz-dir crates/fuzz compare_skip --release

.PHONY: all
all: format lint test test-python
