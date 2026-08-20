//! Parse the json-cases corpus, once as a whole and once per [`TAGS`] tag — which is what says
//! roughly where a change landed, `strings` with the string scanner, `deep` with the recursion,
//! `error` with the failure path. Documents carry several tags, so the sets overlap. See
//! [`corpus`] for the checkout, which this skips entirely when it isn't there; the numbers only
//! compare between runs over the same revision of it, `cases/` being generated.
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
