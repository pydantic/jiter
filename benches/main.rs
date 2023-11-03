use bencher::black_box;
use codspeed_bencher_compat::{benchmark_group, benchmark_main, Bencher};

use std::fs::File;
use std::io::Read;

use jiter::{Jiter, JsonValue, Peak};
use serde_json::Value;

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
        let v = JsonValue::parse(black_box(json_data), false).unwrap();
        black_box(v)
    })
}

fn jiter_iter_big(path: &str, bench: &mut Bencher) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());
    bench.iter(|| {
        let mut jiter = Jiter::new(json_data, false);
        let mut v_outer = Vec::new();
        jiter.next_array().unwrap();

        loop {
            let mut v_inner = Vec::new();
            if let Some(peak) = jiter.next_array().unwrap() {
                let i = jiter.known_float(peak).unwrap();
                v_inner.push(i);
                while let Some(peak) = jiter.array_step().unwrap() {
                    let i = jiter.known_float(peak).unwrap();
                    v_inner.push(i);
                }
            }
            v_outer.push(v_inner);
            if jiter.array_step().unwrap().is_none() {
                break;
            }
        }
        black_box(v_outer)
    })
}

fn find_string(jiter: &mut Jiter) -> String {
    let peak = jiter.peak().unwrap();
    match peak {
        Peak::String => jiter.known_str().unwrap().to_string(),
        Peak::Array => {
            assert!(jiter.known_array().unwrap().is_some());
            let s = find_string(jiter).to_string();
            assert!(jiter.array_step().unwrap().is_none());
            s
        }
        _ => panic!("Expected string or array"),
    }
}

fn jiter_iter_pass2(path: &str, bench: &mut Bencher) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());
    bench.iter(|| {
        let mut jiter = Jiter::new(json_data, false);
        let string = find_string(&mut jiter);
        jiter.finish().unwrap();
        black_box(string)
    })
}

fn jiter_iter_string_array(path: &str, bench: &mut Bencher) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());
    bench.iter(|| {
        let mut jiter = Jiter::new(json_data, false);
        let mut v = Vec::new();
        jiter.next_array().unwrap();
        let i = jiter.known_str().unwrap();
        // record len instead of allocating the string to simulate something like constructing a PyString
        v.push(i.len());
        while jiter.array_step().unwrap().is_some() {
            let i = jiter.known_str().unwrap();
            v.push(i.len());
        }
        jiter.finish().unwrap();
        black_box(v)
    })
}

fn jiter_iter_true_array(path: &str, bench: &mut Bencher) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());
    bench.iter(|| {
        let mut jiter = Jiter::new(json_data, false);
        let mut v = Vec::new();
        let first_peak = jiter.next_array().unwrap().unwrap();
        let i = jiter.known_bool(first_peak).unwrap();
        v.push(i);
        while let Some(peak) = jiter.array_step().unwrap() {
            let i = jiter.known_bool(peak).unwrap();
            v.push(i);
        }
        black_box(v)
    })
}

fn jiter_iter_true_object(path: &str, bench: &mut Bencher) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());
    bench.iter(|| {
        let mut jiter = Jiter::new(json_data, false);
        let mut v = Vec::new();
        if let Some(first_key) = jiter.next_object().unwrap() {
            let first_key = first_key.to_string();
            let first_value = jiter.next_bool().unwrap();
            v.push((first_key, first_value));
            while let Some(key) = jiter.next_key().unwrap() {
                let key = key.to_string();
                let value = jiter.next_bool().unwrap();
                v.push((key, value));
            }
        }
        black_box(v)
    })
}

fn jiter_iter_ints_array(path: &str, bench: &mut Bencher) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());
    bench.iter(|| {
        let mut jiter = Jiter::new(json_data, false);
        let mut v = Vec::new();
        let first_peak = jiter.next_array().unwrap().unwrap();
        let i = jiter.known_int(first_peak).unwrap();
        v.push(i);
        while let Some(peak) = jiter.array_step().unwrap() {
            let i = jiter.known_int(peak).unwrap();
            v.push(i);
        }
        black_box(v)
    })
}

fn jiter_iter_floats_array(path: &str, bench: &mut Bencher) {
    let json = read_file(path);
    let json_data = black_box(json.as_bytes());
    bench.iter(|| {
        let mut jiter = Jiter::new(json_data, false);
        let mut v = Vec::new();
        let first_peak = jiter.next_array().unwrap().unwrap();
        let i = jiter.known_float(first_peak).unwrap();
        v.push(i);
        while let Some(peak) = jiter.array_step().unwrap() {
            let i = jiter.known_float(peak).unwrap();
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
            fn [< $file_name _jiter_value >](bench: &mut Bencher) {
                let file_path = format!("./benches/{}.json", stringify!($file_name));
                jiter_value(&file_path, bench);
            }

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
                    jiter_iter_ints_array(&file_path, bench);
                } else if file_name == "massive_ints_array" {
                    jiter_iter_ints_array(&file_path, bench);
                } else if file_name == "floats_array" {
                    jiter_iter_floats_array(&file_path, bench);
                }
            }

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
test_cases!(massive_ints_array);
test_cases!(floats_array);
// from https://github.com/json-iterator/go-benchmark/blob/179abe5e3f72acce34fb5a16f3473b901fbdd6b9/
// src/github.com/json-iterator/go-benchmark/benchmark.go#L30C17-L30C29
test_cases!(medium_response);

benchmark_group!(
    benches,
    big_jiter_iter,
    big_jiter_value,
    big_serde_value,
    bigints_array_jiter_iter,
    bigints_array_jiter_value,
    bigints_array_serde_value,
    floats_array_jiter_iter,
    floats_array_jiter_value,
    floats_array_serde_value,
    massive_ints_array_jiter_iter,
    massive_ints_array_jiter_value,
    massive_ints_array_serde_value,
    medium_response_jiter_iter,
    medium_response_jiter_value,
    medium_response_serde_value,
    pass1_jiter_iter,
    pass1_jiter_value,
    pass1_serde_value,
    pass2_jiter_iter,
    pass2_jiter_value,
    pass2_serde_value,
    string_array_jiter_iter,
    string_array_jiter_value,
    string_array_serde_value,
    true_array_jiter_iter,
    true_array_jiter_value,
    true_array_serde_value,
    true_object_jiter_iter,
    true_object_jiter_value,
    true_object_serde_value
);
benchmark_main!(benches);
