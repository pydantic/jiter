#![feature(test)]

extern crate test;

use jiter::python_parse;
use std::fs::File;
use std::io::Read;
use test::{black_box, Bencher};

use pyo3::Python;

#[bench]
fn bench_python_parse_numeric(bench: &mut Bencher) {
    Python::with_gil(|py| {
        bench.iter(|| {
            // Clear PyO3 memory on each loop iteration to avoid long GC traversal overheads.
            let _pool = unsafe { py.new_pool() };
            black_box(python_parse(
                py,
                black_box(br#"  { "int": 1, "bigint": 123456789012345678901234567890, "float": 1.2}  "#),
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
            black_box(python_parse(py, black_box(br#"["string", true, false, null]"#))).unwrap()
        });
    })
}

#[bench]
fn test_python_parse_medium_response(bench: &mut Bencher) {
    let mut file = File::open("./benches/medium_response.json").unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();
    let json_data = contents.as_bytes();

    Python::with_gil(|py| {
        bench.iter(|| {
            // Clear PyO3 memory on each loop iteration to avoid long GC traversal overheads.
            let _pool = unsafe { py.new_pool() };
            black_box(python_parse(py, black_box(json_data))).unwrap()
        });
    })
}

#[bench]
fn bench_python_parse_true_object(bench: &mut Bencher) {
    let mut file = File::open("./benches/true_object.json").unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();
    let json_data = contents.as_bytes();

    Python::with_gil(|py| {
        bench.iter(|| {
            // Clear PyO3 memory on each loop iteration to avoid long GC traversal overheads.
            let _pool = unsafe { py.new_pool() };
            black_box(python_parse(py, black_box(json_data)).unwrap())
        });
    })
}
