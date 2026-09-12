use codspeed_criterion_compat::{Criterion, criterion_group, criterion_main};

use std::fs::File;
use std::io::Read;
use std::path::Path;

use pyo3::Python;

use jiter::{PythonParse, StringCacheMode, cache_clear};

fn python_parse_numeric(c: &mut Criterion) {
    Python::attach(|py| {
        cache_clear();
        c.bench_function("python_parse_numeric", |bench| {
            bench.iter(|| {
                PythonParse::default()
                    .python_parse(
                        py,
                        br#"  { "int": 1, "bigint": 123456789012345678901234567890, "float": 1.2}  "#,
                    )
                    .unwrap()
            });
        });
    });
}

fn python_parse_other(c: &mut Criterion) {
    Python::attach(|py| {
        cache_clear();
        c.bench_function("python_parse_other", |bench| {
            bench.iter(|| {
                PythonParse::default()
                    .python_parse(py, br#"["string", true, false, null]"#)
                    .unwrap()
            });
        });
    });
}

fn python_parse_file(path: &str, c: &mut Criterion, cache_mode: StringCacheMode) {
    let path = Path::new(path);
    let mut file = File::open(path).unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();
    let json_data = contents.as_bytes();

    let title = {
        let file_stem = path.file_stem().unwrap().to_str().unwrap();

        let cache_mode = match cache_mode {
            StringCacheMode::None => "_not_cached",
            _ => "",
        };

        "python_parse_".to_owned() + file_stem + cache_mode
    };

    Python::attach(|py| {
        cache_clear();

        c.bench_function(&title, |bench| {
            bench.iter(|| {
                PythonParse {
                    cache_mode,
                    ..Default::default()
                }
                .python_parse(py, json_data)
                .unwrap()
            });
        });
    });
}

fn python_parse_massive_ints_array(c: &mut Criterion) {
    python_parse_file("./benches/massive_ints_array.json", c, StringCacheMode::All);
}

fn python_parse_medium_response_not_cached(c: &mut Criterion) {
    python_parse_file("./benches/medium_response.json", c, StringCacheMode::None);
}

fn python_parse_medium_response(c: &mut Criterion) {
    python_parse_file("./benches/medium_response.json", c, StringCacheMode::All);
}

fn python_parse_true_object_not_cached(c: &mut Criterion) {
    python_parse_file("./benches/true_object.json", c, StringCacheMode::None);
}

fn python_parse_string_array_not_cached(c: &mut Criterion) {
    python_parse_file("./benches/string_array.json", c, StringCacheMode::None);
}

fn python_parse_string_array(c: &mut Criterion) {
    python_parse_file("./benches/string_array.json", c, StringCacheMode::All);
}

/// A thousand ASCII strings of 64 to 2000 characters, too long for the string cache, so one
/// variant covers both cache modes. Built here so the repository doesn't carry a megabyte of filler.
fn long_strings_json() -> Vec<u8> {
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let filler = b"the quick brown fox jumps over the lazy dog 0123456789 ";
    let mut json = Vec::with_capacity(1_100_000);
    json.push(b'[');
    for i in 0..1000 {
        if i > 0 {
            json.push(b',');
        }
        json.push(b'"');
        let len = 64 + (next() % (2000 - 64 + 1)) as usize;
        let offset = (next() % filler.len() as u64) as usize;
        json.extend(filler.iter().cycle().skip(offset).take(len));
        json.push(b'"');
    }
    json.push(b']');
    json
}

fn python_parse_long_strings(c: &mut Criterion) {
    let json_data = long_strings_json();
    Python::attach(|py| {
        cache_clear();
        c.bench_function("python_parse_long_strings", |bench| {
            bench.iter(|| PythonParse::default().python_parse(py, &json_data).unwrap());
        });
    });
}

fn python_parse_string_array_unique_not_cached(c: &mut Criterion) {
    python_parse_file("./benches/string_array_unique.json", c, StringCacheMode::None);
}

fn python_parse_string_array_unique(c: &mut Criterion) {
    python_parse_file("./benches/string_array_unique.json", c, StringCacheMode::All);
}

fn python_parse_true_object(c: &mut Criterion) {
    python_parse_file("./benches/true_object.json", c, StringCacheMode::All);
}

/// Note - caching strings should make no difference here
fn python_parse_true_array(c: &mut Criterion) {
    python_parse_file("./benches/true_array.json", c, StringCacheMode::All);
}

criterion_group!(
    benches,
    python_parse_numeric,
    python_parse_other,
    python_parse_medium_response_not_cached,
    python_parse_medium_response,
    python_parse_true_object_not_cached,
    python_parse_string_array_not_cached,
    python_parse_string_array,
    python_parse_string_array_unique_not_cached,
    python_parse_string_array_unique,
    python_parse_long_strings,
    python_parse_true_object,
    python_parse_true_array,
    python_parse_massive_ints_array,
);
criterion_main!(benches);
