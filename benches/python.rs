use bencher::black_box;
use codspeed_bencher_compat::{benchmark_group, benchmark_main, Bencher};

use jiter::python_parse;
use std::fs::File;
use std::io::Read;

use pyo3::Python;

fn python_parse_numeric(bench: &mut Bencher) {
    Python::with_gil(|py| {
        bench.iter(|| {
            // Clear PyO3 memory on each loop iteration to avoid long GC traversal overheads.
            let _pool = unsafe { py.new_pool() };
            black_box(python_parse(
                py,
                black_box(br#"  { "int": 1, "bigint": 123456789012345678901234567890, "float": 1.2}  "#),
                false,
            ))
            .unwrap()
        });
    })
}

fn python_parse_other(bench: &mut Bencher) {
    Python::with_gil(|py| {
        bench.iter(|| {
            // Clear PyO3 memory on each loop iteration to avoid long GC traversal overheads.
            let _pool = unsafe { py.new_pool() };
            black_box(python_parse(py, black_box(br#"["string", true, false, null]"#), false)).unwrap()
        });
    })
}

fn python_parse_medium_response(bench: &mut Bencher) {
    let mut file = File::open("./benches/medium_response.json").unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();
    let json_data = contents.as_bytes();

    Python::with_gil(|py| {
        bench.iter(|| {
            // Clear PyO3 memory on each loop iteration to avoid long GC traversal overheads.
            let _pool = unsafe { py.new_pool() };
            black_box(python_parse(py, black_box(json_data), false)).unwrap()
        });
    })
}

fn python_parse_true_object(bench: &mut Bencher) {
    let mut file = File::open("./benches/true_object.json").unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();
    let json_data = contents.as_bytes();

    Python::with_gil(|py| {
        bench.iter(|| {
            // Clear PyO3 memory on each loop iteration to avoid long GC traversal overheads.
            let _pool = unsafe { py.new_pool() };
            black_box(python_parse(py, black_box(json_data), false).unwrap())
        });
    })
}

benchmark_group!(
    benches,
    python_parse_numeric,
    python_parse_other,
    python_parse_medium_response,
    python_parse_true_object
);
benchmark_main!(benches);
