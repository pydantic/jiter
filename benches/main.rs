#![feature(test)]
use std::fs::File;
use std::io::Read;

extern crate test;

use test::{black_box, Bencher};
use serde_json::Value;
use donervan::{JsonValue, Chunker, Chunk};

#[bench]
fn donervan_value_pass1(bench: &mut Bencher) {
    // pass1.json is downloaded from https://json.org/JSON_checker/test/pass1.json
    // see https://github.com/python/cpython/blob/main/Lib/test/test_json/test_pass1.py
    let mut f = File::open("./benches/pass1.json").unwrap();
    let mut contents = String::new();
    f.read_to_string(&mut contents).unwrap();

    let json_data = black_box(contents.as_bytes());
    bench.iter(|| {
        let v = JsonValue::parse(json_data).unwrap();
        black_box(v);
    })
}

#[bench]
fn donervan_chunker_parse_pass1(bench: &mut Bencher) {
    let mut f = File::open("./benches/pass1.json").unwrap();
    let mut contents = String::new();
    f.read_to_string(&mut contents).unwrap();

    let json_data = black_box(contents.as_bytes());
    bench.iter(|| {
        let mut chunker = Chunker::new(json_data);
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

#[bench]
fn donervan_chunker_skip_pass1(bench: &mut Bencher) {
    let mut f = File::open("./benches/pass1.json").unwrap();
    let mut contents = String::new();
    f.read_to_string(&mut contents).unwrap();

    let json_data = black_box(contents.as_bytes());
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

#[bench]
fn serde_value_pass1(bench: &mut Bencher) {
    let mut f = File::open("./benches/pass1.json").unwrap();
    let mut contents = String::new();
    f.read_to_string(&mut contents).unwrap();

    let json_data = black_box(contents.as_bytes());
    bench.iter(|| {
        let value: Value = serde_json::from_slice(json_data).unwrap();
        black_box(value);
    })
}
