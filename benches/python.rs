#![feature(test)]

extern crate test;

use std::fs::File;
use std::io::Read;
use test::{black_box, Bencher};

use pyo3::Python;

use jiter::python_parse;

#[bench]
fn bench_python_parse_numeric(bench: &mut Bencher) {
    Python::with_gil(|py| {
        bench.iter(|| {
            // Clear PyO3 memory on each loop iteration to avoid long GC traversal overheads.
            let _pool = unsafe { py.new_pool() };
            black_box(python_parse(
                py,
                black_box(br#"  { "int": 1, "bigint": 123456789012345678901234567890, "float": 1.2}  "#),
                true,
            ))
            .unwrap()
        });
    })
}

#[bench]
fn test_python_parse_other(bench: &mut Bencher) {
    Python::with_gil(|py| {
        bench.iter(|| {
            // Clear PyO3 memory on each loop iteration to avoid long GC traversal overheads.
            let _pool = unsafe { py.new_pool() };
            black_box(python_parse(py, black_box(br#"["string", true, false, null]"#), true)).unwrap()
        });
    })
}

fn _python_parse_file(path: &str, bench: &mut Bencher, cache_strings: bool) {
    let mut file = File::open(path).unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();
    let json_data = contents.as_bytes();

    Python::with_gil(|py| {
        bench.iter(|| {
            // Clear PyO3 memory on each loop iteration to avoid long GC traversal overheads.
            let _pool = unsafe { py.new_pool() };
            black_box(python_parse(py, black_box(json_data), cache_strings)).unwrap()
        });
    })
}
#[bench]
fn test_python_parse_medium_response_cached(bench: &mut Bencher) {
    _python_parse_file("./benches/medium_response.json", bench, true);
}

#[bench]
fn test_python_parse_medium_response_not_cached(bench: &mut Bencher) {
    _python_parse_file("./benches/medium_response.json", bench, false);
}

#[bench]
fn bench_python_parse_true_object_cached(bench: &mut Bencher) {
    _python_parse_file("./benches/true_object.json", bench, true);
}

#[bench]
fn bench_python_parse_true_object_not_cached(bench: &mut Bencher) {
    _python_parse_file("./benches/true_object.json", bench, false);
}

/// Note - caching strings should make no difference here
#[bench]
fn bench_python_parse_true_array_cache(bench: &mut Bencher) {
    _python_parse_file("./benches/true_array.json", bench, true);
}

#[bench]
fn bench_python_parse_true_array_no_cache(bench: &mut Bencher) {
    _python_parse_file("./benches/true_array.json", bench, false);
}
