use codspeed_bencher_compat::{benchmark_group, benchmark_main, Bencher};

use std::fs::File;
use std::io::Read;

use pyo3::Python;

use jiter::python_parse;

fn python_parse_numeric(bench: &mut Bencher) {
    Python::with_gil(|py| {
        bench.iter(|| {
            python_parse(
                py,
                br#"  { "int": 1, "bigint": 123456789012345678901234567890, "float": 1.2}  "#,
                false,
                true,
            )
            .unwrap()
        });
    })
}

fn python_parse_other(bench: &mut Bencher) {
    Python::with_gil(|py| {
        bench.iter(|| python_parse(py, br#"["string", true, false, null]"#, false, true).unwrap());
    })
}

fn _python_parse_file(path: &str, bench: &mut Bencher, cache_strings: bool) {
    let mut file = File::open(path).unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();
    let json_data = contents.as_bytes();

    Python::with_gil(|py| {
        bench.iter(|| python_parse(py, json_data, false, cache_strings).unwrap());
    })
}

fn python_parse_medium_response_not_cached(bench: &mut Bencher) {
    _python_parse_file("./benches/medium_response.json", bench, false);
}

fn python_parse_medium_response(bench: &mut Bencher) {
    _python_parse_file("./benches/medium_response.json", bench, true);
}

fn python_parse_true_object_not_cached(bench: &mut Bencher) {
    _python_parse_file("./benches/true_object.json", bench, false);
}

fn python_parse_string_array_not_cached(bench: &mut Bencher) {
    _python_parse_file("./benches/string_array.json", bench, false);
}

fn python_parse_string_array(bench: &mut Bencher) {
    _python_parse_file("./benches/string_array.json", bench, true);
}

fn python_parse_string_array_unique_not_cached(bench: &mut Bencher) {
    _python_parse_file("./benches/string_array_unique.json", bench, false);
}

fn python_parse_string_array_unique(bench: &mut Bencher) {
    _python_parse_file("./benches/string_array_unique.json", bench, true);
}

fn python_parse_true_object(bench: &mut Bencher) {
    _python_parse_file("./benches/true_object.json", bench, true);
}

/// Note - caching strings should make no difference here
fn python_parse_true_array(bench: &mut Bencher) {
    _python_parse_file("./benches/true_array.json", bench, true);
}

benchmark_group!(
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
    python_parse_true_object,
    python_parse_true_array,
);
benchmark_main!(benches);
