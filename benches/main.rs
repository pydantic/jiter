#![feature(test)]
use std::fs::File;
use std::io::Read;

extern crate test;

use jiter::{Jiter, JsonValue, Peak};
use serde_json::Value;
use test::{black_box, Bencher};

fn read_file(path: &str) -> String {
    let mut file = File::open(path).unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();
    contents
}

fn jiter_value(path: &str, bench: &mut Bencher) {
    let json = read_file(path);
    let json_data = json.as_bytes();
    bench.iter(|| {
        let v = JsonValue::parse(black_box(json_data)).unwrap();
        black_box(v)
    })
}

fn jiter_iter_big(path: &str, bench: &mut Bencher) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());
    bench.iter(|| {
        let mut jiter = Jiter::new(json_data);
        let mut v_outer = Vec::new();
        jiter.array_first().unwrap();

        loop {
            let mut v_inner = Vec::new();
            if jiter.array_first().unwrap().is_some() {
                let i = jiter.next_float().unwrap();
                v_inner.push(i);
                while jiter.array_step().unwrap() {
                    let i = jiter.next_float().unwrap();
                    v_inner.push(i);
                }
            }
            v_outer.push(v_inner);
            if !jiter.array_step().unwrap() {
                break;
            }
        }
        black_box(v_outer)
    })
}

fn find_string(jiter: &mut Jiter) -> String {
    let peak = jiter.peak().unwrap();
    match peak {
        Peak::String => jiter.known_string().unwrap(),
        Peak::Array => {
            assert!(jiter.array_first().unwrap().is_some());
            let s = find_string(jiter);
            assert!(!jiter.array_step().unwrap());
            s
        }
        _ => panic!("Expected string or array"),
    }
}

fn jiter_iter_pass2(path: &str, bench: &mut Bencher) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());
    bench.iter(|| {
        let mut jiter = Jiter::new(json_data);
        let string = find_string(&mut jiter);
        jiter.finish().unwrap();
        black_box(string)
    })
}

fn jiter_iter_string_array(path: &str, bench: &mut Bencher) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());
    bench.iter(|| {
        let mut jiter = Jiter::new(json_data);
        let mut v = Vec::new();
        jiter.array_first().unwrap();
        let i = jiter.next_str().unwrap();
        v.push(i);
        while jiter.array_step().unwrap() {
            let i = jiter.next_str().unwrap();
            v.push(i);
        }
        jiter.finish().unwrap();
        black_box(v)
    })
}

fn jiter_iter_true_array(path: &str, bench: &mut Bencher) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());
    bench.iter(|| {
        let mut jiter = Jiter::new(json_data);
        let mut v = Vec::new();
        jiter.array_first().unwrap();
        let i = jiter.next_bool().unwrap();
        v.push(i);
        while jiter.array_step().unwrap() {
            let i = jiter.next_bool().unwrap();
            v.push(i);
        }
        black_box(v)
    })
}

fn jiter_iter_true_object(path: &str, bench: &mut Bencher) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());
    bench.iter(|| {
        let mut jiter = Jiter::new(json_data);
        let mut v = Vec::new();
        if let Some(first_key) = jiter.next_object().unwrap() {
            let first_value = jiter.next_bool().unwrap();
            v.push((first_key, first_value));
            while let Some(key) = jiter.next_key().unwrap() {
                let value = jiter.next_bool().unwrap();
                v.push((key, value));
            }
        }
        black_box(v)
    })
}

fn jiter_iter_bigints_array(path: &str, bench: &mut Bencher) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());
    bench.iter(|| {
        let mut jiter = Jiter::new(json_data);
        let mut v = Vec::new();
        jiter.array_first().unwrap();
        let i = jiter.next_int().unwrap();
        v.push(i);
        while jiter.array_step().unwrap() {
            let i = jiter.next_int().unwrap();
            v.push(i);
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
            fn [< $file_name _jiter_value >](bench: &mut Bencher) {
                let file_path = format!("./benches/{}.json", stringify!($file_name));
                jiter_value(&file_path, bench);
            }

            #[bench]
            fn [< $file_name _jiter_iter >](bench: &mut Bencher) {
                let file_name = stringify!($file_name);
                let file_path = format!("./benches/{}.json", file_name);
                if file_name == "big" {
                    jiter_iter_big(&file_path, bench);
                } else if file_name == "pass2" {
                    jiter_iter_pass2(&file_path, bench);
                } else if file_name == "string_array" {
                    jiter_iter_string_array(&file_path, bench);
                } else if file_name == "true_array" {
                    jiter_iter_true_array(&file_path, bench);
                } else if file_name == "true_object" {
                    jiter_iter_true_object(&file_path, bench);
                } else if file_name == "bigints_array" {
                    jiter_iter_bigints_array(&file_path, bench);
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
// from https://github.com/json-iterator/go-benchmark/blob/179abe5e3f72acce34fb5a16f3473b901fbdd6b9/
// src/github.com/json-iterator/go-benchmark/benchmark.go#L30C17-L30C29
test_cases!(medium_response);
