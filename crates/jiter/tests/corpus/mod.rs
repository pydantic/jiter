//! Loading the [`json-cases`](https://github.com/samuelcolvin/json-cases) corpus, shared by the
//! `json_cases` test and the `json_cases` benchmark — each uses a subset of what is here.
//!
//! The corpus is a separate checkout whose `cases/` directory is a build artifact.
//! `../json-cases` next to this repository is used by default, `JSON_CASES` overrides it:
//!
//! ```bash
//! git clone https://github.com/samuelcolvin/json-cases ../json-cases
//! make -C ../json-cases build              # writes cases/ and cases.json
//! ```
//!
//! The tests require it: a corpus that isn't there fails them rather than skipping them, so that a
//! checkout without it can't quietly pass the suite. `JSON_CASES_SKIP` is the way to opt out where
//! building the corpus isn't worth it — CI sets it on the job that resolves fresh dependencies.
#![allow(dead_code, clippy::print_stdout)]

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// One entry of `cases.json`. The index also records the error serde_json gave when it was
/// written; that is ignored here, the test runs its own copy of serde_json instead.
#[derive(Deserialize)]
struct Case {
    /// absolute path, valid only in the checkout that generated the index
    path: String,
    /// the top level directory under `cases/`
    category: String,
    /// what the document is made of, see [`TAGS`]
    #[serde(default)]
    tags: Vec<String>,
    /// the other files holding this same document in a different spelling, recorded on one member
    /// of each group so that following it from every entry visits each group once
    #[serde(default)]
    similar: Vec<String>,
}

/// A case with its content read, and its paths re-rooted at this checkout.
pub struct Loaded {
    /// path relative to the corpus root, doubling as the name to report the case by
    pub name: String,
    pub json_data: Vec<u8>,
    pub category: String,
    /// what the document is made of, see [`TAGS`]
    pub tags: Vec<String>,
    /// the `name`s of the other members of this case's group, see [`Case::similar`]
    pub similar: Vec<String>,
}

/// The `json-cases` checkout, or `None` if it hasn't been cloned and built.
pub fn find_corpus_root() -> Option<PathBuf> {
    let root = match std::env::var_os("JSON_CASES") {
        Some(dir) => PathBuf::from(dir),
        None => Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../json-cases"),
    };
    root.join("cases.json").is_file().then_some(root)
}

/// The `json-cases` checkout to run the tests over, or `None` if `JSON_CASES_SKIP` is set to
/// something non-empty; without it a missing corpus panics rather than skipping.
pub fn corpus_root() -> Option<PathBuf> {
    if std::env::var_os("JSON_CASES_SKIP").is_some_and(|skip| !skip.is_empty()) {
        println!("JSON_CASES_SKIP is set, skipping");
        return None;
    }
    Some(find_corpus_root().unwrap_or_else(|| {
        panic!(
            "the json-cases corpus is missing, the tests need it:\n\
             \x20   git clone https://github.com/samuelcolvin/json-cases ../json-cases\n\
             \x20   make -C ../json-cases build\n\
             or point JSON_CASES at an existing checkout; `cases.json` was looked for under {}",
            std::env::var_os("JSON_CASES").map_or_else(
                || "../json-cases, next to this repository".to_string(),
                |dir| dir.to_string_lossy().into_owned()
            )
        )
    }))
}

/// `cases.json` holds absolute paths, valid only in the checkout that generated it, so re-root
/// them; the result doubles as the name to report a case by.
fn relative(root: &Path, path: &str) -> String {
    if let Ok(rel) = Path::new(path).strip_prefix(root) {
        return rel.to_string_lossy().into_owned();
    }
    match path.find("/cases/") {
        Some(index) => path[index + 1..].to_string(),
        None => path.to_string(),
    }
}

/// Every case in the index, with its content read into memory.
pub fn load_cases(root: &Path) -> Vec<Loaded> {
    let index = std::fs::read(root.join("cases.json")).unwrap();
    let cases: Vec<Case> = serde_json::from_slice(&index).unwrap();
    assert!(!cases.is_empty(), "cases.json is empty, run `make build` in the corpus");
    cases
        .into_iter()
        .map(|case| {
            let name = relative(root, &case.path);
            let json_data = std::fs::read(root.join(&name))
                .unwrap_or_else(|e| panic!("{name}: {e}, run `make build` in the corpus"));
            let similar = case.similar.iter().map(|path| relative(root, path)).collect();
            Loaded {
                name,
                json_data,
                category: case.category,
                tags: case.tags,
                similar,
            }
        })
        .collect()
}

/// The `tags` the index uses, in the order the benchmark reports them.
///
/// A tag names a lexical class that holds at least a quarter of the document's bytes — a tenth of
/// its *string* bytes for `escapes` and `non-ascii`, since the escape-free fast path is a fork
/// inside the string scanner rather than a share of the file — so it says what a parser would
/// spend its time on rather than what the document merely contains. `deep` is nesting of 20
/// levels or more, and `error` is a verdict rather than a shape: no conforming parser accepts the
/// document. Documents carry several, and `cases.json` describes them in full.
pub const TAGS: [&str; 12] = [
    "strings",
    "escapes",
    "non-ascii",
    "numbers",
    "ints",
    "floats",
    "constants",
    "arrays",
    "objects",
    "deep",
    "whitespace",
    "error",
];
