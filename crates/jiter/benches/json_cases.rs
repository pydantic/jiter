//! Parse the [`json-cases`](https://github.com/samuelcolvin/json-cases) corpus, the same documents
//! the `json_cases` test compares against serde_json. Every file is read into memory first, then
//! one iteration parses a whole set of them into values and swallows the errors, so unlike the
//! benchmarks in `main.rs` — each a single well formed document of one shape — this measures a
//! mixed workload of many documents, most of them small, over half of them malformed. It is the
//! only benchmark here that exercises the error paths.
//!
//! The corpus is timed once as a whole and then once per [`TAGS`] tag, which is what says roughly
//! where a change landed: `strings` and `escapes` move with the string scanner, `ints`/`floats`
//! with number decoding, `deep` with the recursion, `whitespace` with the between-token loop, and
//! `error` with the failure path. A document carries several tags, so the sets overlap and add up
//! to more than the corpus.
//!
//! The corpus is a separate checkout, see [`corpus`]; when it isn't there this registers no
//! benchmarks at all and `cargo bench` just runs the rest.
//!
//! ```bash
//! git clone https://github.com/samuelcolvin/json-cases ../json-cases
//! make -C ../json-cases build              # writes cases/ and cases.json
//! cargo bench -p jiter --bench json_cases  # or JSON_CASES=/path/to/json-cases cargo bench ...
//! ```
//!
//! `cases/` is generated, so these numbers are only comparable between runs over the same revision
//! of the corpus: regenerating it with a different `json-cases` changes the workload itself.
#![allow(clippy::print_stdout)]

use std::hint::black_box;

use codspeed_criterion_compat::{Criterion, criterion_group, criterion_main};

use jiter::JsonValue;
use serde_json::Value as SerdeValue;

#[path = "../tests/corpus/mod.rs"]
mod corpus;

use corpus::{TAGS, find_corpus_root, load_cases};

/// Parse every document with jiter, counting the ones that parsed — the malformed documents are
/// part of the workload, an error is a result like any other.
fn jiter_value(documents: &[&[u8]]) -> usize {
    let mut parsed = 0;
    for json_data in documents {
        if let Ok(value) = JsonValue::parse(black_box(json_data), false) {
            black_box(&value);
            parsed += 1;
        }
    }
    parsed
}

/// The same with serde_json, for comparison. Note that this crate's dev-dependency on serde_json
/// turns on `arbitrary_precision`, `preserve_order` and `float_roundtrip`, as the benchmarks in
/// `main.rs` do.
fn serde_value(documents: &[&[u8]]) -> usize {
    let mut parsed = 0;
    for json_data in documents {
        if let Ok(value) = serde_json::from_slice::<SerdeValue>(black_box(json_data)) {
            black_box(&value);
            parsed += 1;
        }
    }
    parsed
}

fn corpus_benches(c: &mut Criterion) {
    let Some(root) = find_corpus_root() else {
        println!("json-cases corpus not found, registering no benchmarks (see the module docs)");
        return;
    };
    let cases = load_cases(&root);

    // the whole corpus first, then the documents behind each tag, so a change can be traced to the
    // part of the parser that caused it rather than just to the total
    let all: Vec<&[u8]> = cases.iter().map(|case| case.json_data.as_slice()).collect();
    let mut groups: Vec<(&str, Vec<&[u8]>)> = vec![("all", all)];
    for tag in TAGS {
        let documents: Vec<&[u8]> = cases
            .iter()
            .filter(|case| case.tags.iter().any(|case_tag| case_tag == tag))
            .map(|case| case.json_data.as_slice())
            .collect();
        assert!(
            !documents.is_empty(),
            "no cases tagged {tag}, is the corpus up to date? run `make build` in it"
        );
        groups.push((tag, documents));
    }

    for (name, documents) in &groups {
        let bytes: usize = documents.iter().map(|json_data| json_data.len()).sum();
        println!("json_cases_{name}: {} documents, {} bytes", documents.len(), bytes);
        c.bench_function(&format!("json_cases_{name}_jiter_value"), |bench| {
            bench.iter(|| black_box(jiter_value(documents)));
        });
        c.bench_function(&format!("json_cases_{name}_serde_value"), |bench| {
            bench.iter(|| black_box(serde_value(documents)));
        });
    }
}

criterion_group!(benches, corpus_benches);
criterion_main!(benches);
