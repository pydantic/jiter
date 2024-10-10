use codspeed_bencher_compat::{benchmark_group, benchmark_main, Bencher};
use std::hint::black_box;

use std::fs::File;
use std::io::Read;

use batson::get::{get_str, BatsonPath};
use batson::{batson_to_json_string, encode_from_json};
use jiter::JsonValue;

fn read_file(path: &str) -> String {
    let mut file = File::open(path).unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();
    contents
}

/// taken from <https://github.com/datafusion-contrib/datafusion-functions-json/blob/v0.41.0/src/common.rs#L184-L216>
mod jiter_find {
    use jiter::{Jiter, Peek};

    #[derive(Debug)]
    pub enum JsonPath<'s> {
        Key(&'s str),
        Index(usize),
        None,
    }

    impl From<u64> for JsonPath<'_> {
        fn from(index: u64) -> Self {
            JsonPath::Index(usize::try_from(index).unwrap())
        }
    }

    impl From<i32> for JsonPath<'_> {
        fn from(index: i32) -> Self {
            match usize::try_from(index) {
                Ok(i) => Self::Index(i),
                Err(_) => Self::None,
            }
        }
    }

    impl<'s> From<&'s str> for JsonPath<'s> {
        fn from(key: &'s str) -> Self {
            JsonPath::Key(key)
        }
    }

    pub fn jiter_json_find<'j>(opt_json: Option<&'j str>, path: &[JsonPath]) -> Option<(Jiter<'j>, Peek)> {
        let json_str = opt_json?;
        let mut jiter = Jiter::new(json_str.as_bytes());
        let mut peek = jiter.peek().ok()?;
        for element in path {
            match element {
                JsonPath::Key(key) if peek == Peek::Object => {
                    let mut next_key = jiter.known_object().ok()??;

                    while next_key != *key {
                        jiter.next_skip().ok()?;
                        next_key = jiter.next_key().ok()??;
                    }

                    peek = jiter.peek().ok()?;
                }
                JsonPath::Index(index) if peek == Peek::Array => {
                    let mut array_item = jiter.known_array().ok()??;

                    for _ in 0..*index {
                        jiter.known_skip(array_item).ok()?;
                        array_item = jiter.array_step().ok()??;
                    }

                    peek = array_item;
                }
                _ => {
                    return None;
                }
            }
        }
        Some((jiter, peek))
    }

    pub fn get_str(json_data: Option<&str>, path: &[JsonPath]) -> Option<String> {
        if let Some((mut jiter, peek)) = jiter_json_find(json_data, path) {
            match peek {
                Peek::String => Some(jiter.known_str().ok()?.to_owned()),
                _ => None,
            }
        } else {
            None
        }
    }
}

mod serde_find {
    use batson::get::BatsonPath;
    use serde_json::Value;

    pub fn get_str(json_data: &[u8], path: &[BatsonPath]) -> Option<String> {
        let json_value: Value = serde_json::from_slice(json_data).ok()?;
        let mut current = &json_value;
        for key in path {
            current = match (key, current) {
                (BatsonPath::Key(k), Value::Object(map)) => map.get(*k)?,
                (BatsonPath::Index(i), Value::Array(vec)) => vec.get(*i)?,
                _ => return None,
            }
        }
        match current {
            Value::String(s) => Some(s.clone()),
            _ => None,
        }
    }
}

fn json_to_batson(json: &[u8]) -> Vec<u8> {
    let json_value = JsonValue::parse(json, false).unwrap();
    encode_from_json(&json_value).unwrap()
}

fn medium_get_str_found_batson(bench: &mut Bencher) {
    let json = read_file("../jiter/benches/medium_response.json");
    let json_data = json.as_bytes();
    let batson_data = json_to_batson(json_data);
    let path: Vec<BatsonPath> = vec!["person".into(), "linkedin".into(), "handle".into()];
    bench.iter(|| {
        let v = get_str(black_box(&batson_data), &path);
        black_box(v)
    });
}

fn medium_get_str_found_jiter(bench: &mut Bencher) {
    let json = read_file("../jiter/benches/medium_response.json");
    let path: Vec<jiter_find::JsonPath> = vec!["person".into(), "linkedin".into(), "handle".into()];
    bench.iter(|| {
        let v = jiter_find::get_str(black_box(Some(&json)), &path);
        black_box(v)
    });
}

fn medium_get_str_found_serde(bench: &mut Bencher) {
    let json = read_file("../jiter/benches/medium_response.json");
    let json_data = json.as_bytes();
    let path: Vec<BatsonPath> = vec!["person".into(), "linkedin".into(), "handle".into()];
    bench.iter(|| {
        let v = serde_find::get_str(black_box(json_data), &path).unwrap();
        black_box(v)
    });
}

fn medium_get_str_missing_batson(bench: &mut Bencher) {
    let json = read_file("../jiter/benches/medium_response.json");
    let json_data = json.as_bytes();
    let batson_data = json_to_batson(json_data);
    let path: Vec<BatsonPath> = vec!["squid".into(), "linkedin".into(), "handle".into()];
    bench.iter(|| {
        let v = get_str(black_box(&batson_data), &path);
        black_box(v)
    });
}

fn medium_get_str_missing_jiter(bench: &mut Bencher) {
    let json = read_file("../jiter/benches/medium_response.json");
    let path: Vec<jiter_find::JsonPath> = vec!["squid".into(), "linkedin".into(), "handle".into()];
    bench.iter(|| {
        let v = jiter_find::get_str(black_box(Some(&json)), &path);
        black_box(v)
    });
}

fn medium_get_str_missing_serde(bench: &mut Bencher) {
    let json = read_file("../jiter/benches/medium_response.json");
    let json_data = json.as_bytes();
    let path: Vec<BatsonPath> = vec!["squid".into(), "linkedin".into(), "handle".into()];
    bench.iter(|| {
        let v = serde_find::get_str(black_box(json_data), &path);
        black_box(v)
    });
}

fn medium_convert_batson_to_json(bench: &mut Bencher) {
    let json = read_file("../jiter/benches/medium_response.json");
    let json_data = json.as_bytes();
    let batson_data = json_to_batson(json_data);
    bench.iter(|| {
        let v = batson_to_json_string(black_box(&batson_data)).unwrap();
        black_box(v)
    });
}

fn medium_convert_json_to_batson(bench: &mut Bencher) {
    let json = read_file("../jiter/benches/medium_response.json");
    let json = json.as_bytes();
    bench.iter(|| {
        let json_value = JsonValue::parse(json, false).unwrap();
        let b = encode_from_json(&json_value).unwrap();
        black_box(b)
    });
}

benchmark_group!(
    benches,
    medium_get_str_found_batson,
    medium_get_str_found_jiter,
    medium_get_str_found_serde,
    medium_get_str_missing_batson,
    medium_get_str_missing_jiter,
    medium_get_str_missing_serde,
    medium_convert_batson_to_json,
    medium_convert_json_to_batson
);
benchmark_main!(benches);
