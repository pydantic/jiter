#![feature(test)]
use std::fs::File;
use std::io::Read;

extern crate test;

use donervan::{Decoder, Element, JsonValue, Parser, Fleece};
use serde_json::Value;
use test::{black_box, Bencher};

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

fn donervan_parser_parse(path: &str, bench: &mut Bencher) {
    let json = read_file(path);
    let json_data = json.as_bytes();
    let decoder = Decoder::new(json_data);
    bench.iter(|| {
        let mut parser = Parser::new(black_box(json_data));
        while let Some(element_result) = parser.next() {
            let element = element_result.unwrap();
            match element.element {
                Element::True => {
                    black_box(true);
                }
                Element::False => {
                    black_box(false);
                }
                Element::Null => (),
                Element::String(range) => {
                    let s = decoder.decode_string(range, element.loc).unwrap();
                    black_box(s);
                }
                Element::Int {
                    positive,
                    range,
                    exponent,
                } => {
                    let i = decoder.decode_int(positive, range, exponent, element.loc).unwrap();
                    black_box(i);
                }
                Element::Float {
                    positive,
                    int_range,
                    decimal_range,
                    exponent,
                } => {
                    let f = decoder
                        .decode_float(positive, int_range, decimal_range, exponent, element.loc)
                        .unwrap();
                    black_box(f);
                }
                _ => (),
            }
        }
    })
}

fn donervan_parse_skip(path: &str, bench: &mut Bencher) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());
    bench.iter(|| {
        let mut parser = Parser::new(json_data);
        while let Some(element_result) = parser.next() {
            let element = element_result.unwrap();
            match element.element {
                Element::True => black_box("t"),
                Element::False => black_box("f"),
                Element::Null => black_box("n"),
                Element::String(_) => black_box("s"),
                Element::Int { .. } => black_box("i"),
                Element::Float { .. } => black_box("f"),
                Element::ObjectStart => black_box("x"),
                Element::ObjectEnd => black_box("x"),
                Element::ArrayStart => black_box("x"),
                Element::ArrayEnd => black_box("x"),
                Element::Key(_) => black_box("k"),
            };
        }
    })
}

fn donervan_fleece_string_array(path: &str, bench: &mut Bencher) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());
    bench.iter(|| {
        let mut fleece = Fleece::new(json_data);
        fleece.next_array().unwrap();
        let mut v = Vec::new();
        loop {
            let i = fleece.next_str().unwrap();
            v.push(i);
            if !fleece.array_step().unwrap() {
                break;
            }
        }
        black_box(v)
    })
}

fn donervan_fleece_true_array(path: &str, bench: &mut Bencher) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());
    bench.iter(|| {
        let mut fleece = Fleece::new(json_data);
        fleece.next_array().unwrap();
        let mut v = Vec::new();
        loop {
            let i = fleece.next_bool_strict().unwrap();
            v.push(i);
            if !fleece.array_step().unwrap() {
                break;
            }
        }
        black_box(v)
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
            fn [< $file_name _donervan >](bench: &mut Bencher) {
                let file_path = format!("./benches/{}.json", stringify!($file_name));
                donervan_parser_parse(&file_path, bench);
            }

            #[bench]
            fn [< $file_name _donervan_value >](bench: &mut Bencher) {
                let file_path = format!("./benches/{}.json", stringify!($file_name));
                donervan_value(&file_path, bench);
            }

            #[bench]
            fn [< $file_name _donervan_parse_skip >](bench: &mut Bencher) {
                let file_path = format!("./benches/{}.json", stringify!($file_name));
                donervan_parse_skip(&file_path, bench);
            }

            #[bench]
            fn [< $file_name _donervan_fleece >](bench: &mut Bencher) {
                let file_name = stringify!($file_name);
                let file_path = format!("./benches/{}.json", file_name);
                if file_name == "string_array" {
                    donervan_fleece_string_array(&file_path, bench);
                } else if file_name == "true_array" {
                    donervan_fleece_true_array(&file_path, bench);
                }
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
// this needs ./benches/generate_big.py to be called
test_cases!(big);
// https://json.org/JSON_checker/test/pass2.json
test_cases!(pass2);
test_cases!(string_array);
test_cases!(true_array);
test_cases!(true_object);
