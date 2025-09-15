use codspeed_criterion_compat::{criterion_group, criterion_main, Criterion};

use std::fs::File;
use std::io::Read;
use std::path::Path;

use pyo3::Python;

use jiter::{cache_clear, PythonParse, StringCacheMode};

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

fn python_parse_x100_not_cached(c: &mut Criterion) {
    python_parse_file("./benches/x100.json", c, StringCacheMode::None);
}

fn python_parse_x100(c: &mut Criterion) {
    python_parse_file("./benches/x100.json", c, StringCacheMode::All);
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
    python_parse_x100_not_cached,
    python_parse_x100,
    python_parse_true_object,
    python_parse_true_array,
    python_parse_massive_ints_array,
);
criterion_main!(benches);
