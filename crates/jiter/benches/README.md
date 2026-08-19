# Benchmarks

Before running benchmarks, make sure to generate `big.json`:

```shell
python3 ./crates/jiter/benches/generate_big.py
```

To run benchmarks, run:

```shell
cargo bench -p jiter
```

## The json-cases corpus

`json_cases.rs` is a second benchmark target that parses the whole
[`json-cases`](https://github.com/samuelcolvin/json-cases) corpus — ~2800 documents, most of them
small and over half of them malformed — one iteration per pass over the lot. It complements the
benchmarks in `main.rs`, each of which is one large well formed document of a single shape: this is
the only one that measures the error paths, and the only one whose per-document overhead shows up.

The corpus is a separate checkout, so the benchmark registers nothing at all when it isn't there:

```shell
git clone https://github.com/samuelcolvin/json-cases ../json-cases
make -C ../json-cases build              # writes cases/ and cases.json
cargo bench -p jiter --bench json_cases  # or JSON_CASES=/path/to/json-cases cargo bench ...
```

It reports the corpus as a whole (`json_cases_all_*`) and then one benchmark per tag from
`cases.json` — `json_cases_strings_*`, `json_cases_escapes_*`, `json_cases_ints_*`,
`json_cases_deep_*`, `json_cases_error_*` and so on — which is what says roughly where a change
landed: the string scanner, number decoding, the recursion, the whitespace loop or the failure
path. A tag is only set when that class holds a quarter of the document's bytes (a tenth of its
string bytes for `escapes` and `non-ascii`), so the sets are the documents a parser would really
spend that time on; documents carry several tags, so the sets overlap and add up to more than the
corpus.

`cases/` is generated, so the numbers are only comparable between runs over the same revision of the
corpus — regenerating it with a different `json-cases` changes the workload itself, which is also
why this isn't part of the CodSpeed run in CI.
