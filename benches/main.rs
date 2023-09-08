#![feature(test)]
use std::fs::File;
use std::io::Read;

extern crate test;

use donervan::{Fleece, JsonValue, Peak};
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
        black_box(v)
    })
}

fn donervan_fleece_big(path: &str, bench: &mut Bencher) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());
    bench.iter(|| {
        let mut fleece = Fleece::new(json_data);
        fleece.next_array().unwrap();
        let mut v_outer = Vec::new();
        loop {
            let mut v_inner = Vec::new();
            if fleece.next_array().unwrap() {
                loop {
                    let i = fleece.next_float().unwrap();
                    v_inner.push(i);
                    if !fleece.array_step().unwrap() {
                        break;
                    }
                }
            }
            v_outer.push(v_inner);
            if !fleece.array_step().unwrap() {
                break;
            }
        }
        black_box(v_outer)
    })
}

fn find_string(fleece: &mut Fleece) -> String {
    let peak = fleece.peak().unwrap();
    match peak {
        Peak::String => fleece.known_string().unwrap(),
        Peak::Array => {
            assert!(fleece.array_first().unwrap());
            let s = find_string(fleece);
            assert!(!fleece.array_step().unwrap());
            s
        }
        _ => panic!("Expected string or array"),
    }
}

fn donervan_fleece_pass2(path: &str, bench: &mut Bencher) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());
    bench.iter(|| {
        let mut fleece = Fleece::new(json_data);
        let string = find_string(&mut fleece);
        fleece.finish().unwrap();
        black_box(string)
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
        fleece.finish().unwrap();
        black_box(v)
    })
}

fn donervan_fleece_true_array(path: &str, bench: &mut Bencher) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());
    bench.iter(|| {
        let mut fleece = Fleece::new(json_data);
        let mut v = Vec::new();
        if fleece.next_array().unwrap() {
            loop {
                let i = fleece.next_bool().unwrap();
                v.push(i);
                if !fleece.array_step().unwrap() {
                    break;
                }
            }
        }
        black_box(v)
    })
}

fn donervan_fleece_true_object(path: &str, bench: &mut Bencher) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());
    bench.iter(|| {
        let mut fleece = Fleece::new(json_data);
        let mut v = Vec::new();
        if let Some(first_key) = fleece.next_object().unwrap() {
            let first_value = fleece.next_bool().unwrap();
            v.push((first_key, first_value));
            while let Some(key) = fleece.next_key().unwrap() {
                let value = fleece.next_bool().unwrap();
                v.push((key, value));
            }
        }
        black_box(v)
    })
}

fn donervan_fleece_bigints_array(path: &str, bench: &mut Bencher) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());
    bench.iter(|| {
        let mut fleece = Fleece::new(json_data);
        let mut v = Vec::new();
        if fleece.next_array().unwrap() {
            loop {
                let i = fleece.next_int().unwrap();
                v.push(i);
                if !fleece.array_step().unwrap() {
                    break;
                }
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
            fn [< $file_name _donervan_value >](bench: &mut Bencher) {
                let file_path = format!("./benches/{}.json", stringify!($file_name));
                donervan_value(&file_path, bench);
            }

            #[bench]
            fn [< $file_name _donervan_fleece >](bench: &mut Bencher) {
                let file_name = stringify!($file_name);
                let file_path = format!("./benches/{}.json", file_name);
                if file_name == "big" {
                    donervan_fleece_big(&file_path, bench);
                } else if file_name == "pass2" {
                    donervan_fleece_pass2(&file_path, bench);
                } else if file_name == "string_array" {
                    donervan_fleece_string_array(&file_path, bench);
                } else if file_name == "true_array" {
                    donervan_fleece_true_array(&file_path, bench);
                } else if file_name == "true_object" {
                    donervan_fleece_true_object(&file_path, bench);
                } else if file_name == "bigints_array" {
                    donervan_fleece_bigints_array(&file_path, bench);
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
test_cases!(bigints_array);
