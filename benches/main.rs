#![feature(test)]
use std::fs::File;
use std::io::Read;

extern crate test;

use test::{black_box, Bencher};
use serde_json::Value;
use donervan::{JsonValue, Chunker, Chunk};

fn read_file(path: &str) -> String {
    let mut file = File::open(path).unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();
    contents
}

fn donervan_value(path: &str, bench: &mut Bencher) {
    let json = read_file(path);
    let json_data = json.as_bytes();
    bench.iter(|| {
        let v = JsonValue::parse(black_box(json_data)).unwrap();
        black_box(v);
    })
}

fn donervan_chunker_parse(path: &str, bench: &mut Bencher) {
    let json = read_file(path);
    let json_data = json.as_bytes();
    bench.iter(|| {
        let mut chunker = Chunker::new(black_box(json_data));
        loop {
            let chunk = match chunker.next() {
                Some(c) => c.unwrap(),
                _ => break,
            };
            match chunk.chunk_type {
                Chunk::True => {
                    black_box(true);
                    ()
                },
                Chunk::False => {
                    black_box(false);
                    ()
                }
                Chunk::Null => (),
                Chunk::String(range) => {
                    let s = chunker.decode_string(range, chunk.loc).unwrap();
                    black_box(s);
                    ()
                }
                Chunk::Int {
                    positive,
                    range,
                    exponent,
                } => {
                    let i = chunker.decode_int(positive, range, exponent, chunk.loc).unwrap();
                    black_box(i);
                    ()
                }
                Chunk::Float {
                    positive,
                    int_range,
                    decimal_range,
                    exponent,
                } => {
                    let f = chunker.decode_float(positive, int_range, decimal_range, exponent, chunk.loc).unwrap();
                    black_box(f);
                    ()
                }
                _ => ()
            }
        }
    })
}

fn donervan_chunker_skip(path: &str, bench: &mut Bencher) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());
    bench.iter(|| {
        let mut chunker = Chunker::new(json_data);
        loop {
            let chunk = match chunker.next() {
                Some(c) => c.unwrap(),
                _ => break,
            };
            match chunk.chunk_type {
                Chunk::True => black_box("t"),
                Chunk::False => black_box("f"),
                Chunk::Null => black_box("n"),
                Chunk::String(_) => black_box("s"),
                Chunk::Int { .. } => black_box("i"),
                Chunk::Float { .. } => black_box("f"),
                Chunk::ObjectStart => black_box("x"),
                Chunk::ObjectEnd => black_box("x"),
                Chunk::ArrayStart => black_box("x"),
                Chunk::ArrayEnd => black_box("x"),
            };
        }
    })
}

fn serde_value(path: &str, bench: &mut Bencher) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());
    bench.iter(|| {
        let value: Value = serde_json::from_slice(json_data).unwrap();
        black_box(value);
    })
}

macro_rules! test_cases {
    ($file_name:ident) => {
        paste::item! {
            #[bench]
            fn [< $file_name _donervan_value >](bench: &mut Bencher) {
                let file_path = format!("./benches/{}.json", stringify!($file_name));
                donervan_value(&file_path, bench);
            }

            #[bench]
            fn [< $file_name _donervan_chunker_parse >](bench: &mut Bencher) {
                let file_path = format!("./benches/{}.json", stringify!($file_name));
                donervan_chunker_parse(&file_path, bench);
            }

            #[bench]
            fn [< $file_name _donervan_chunker_skip >](bench: &mut Bencher) {
                let file_path = format!("./benches/{}.json", stringify!($file_name));
                donervan_chunker_skip(&file_path, bench);
            }

            #[bench]
            fn [< $file_name _serde_value >](bench: &mut Bencher) {
                let file_path = format!("./benches/{}.json", stringify!($file_name));
                serde_value(&file_path, bench);
            }
        }
    };
}

// https://json.org/JSON_checker/test/pass1.json
// see https://github.com/python/cpython/blob/main/Lib/test/test_json/test_pass1.py
test_cases!(pass1);
// https://json.org/JSON_checker/test/pass2.json
test_cases!(pass2);
test_cases!(string_array);
test_cases!(true_array);
test_cases!(true_object);
