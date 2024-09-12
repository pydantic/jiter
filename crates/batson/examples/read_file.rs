use batson::get::BatsonPath;
use batson::{batson_to_json_string, encode_from_json};
use jiter::JsonValue;
use std::fs::File;
use std::io::Read;

fn main() {
    let filename = std::env::args().nth(1).expect(
        r#"
No arguments provided!

Usage:
cargo run --example read_file file.json [path]
"#,
    );

    let mut file = File::open(&filename).expect("failed to open file");
    let mut json = Vec::new();
    file.read_to_end(&mut json).expect("failed to read file");

    let json_value = JsonValue::parse(&json, false).expect("invalid JSON");
    let batson = encode_from_json(&json_value).expect("failed to construct batson data");
    println!("json length: {}", json.len());
    println!("batson length: {}", batson.len());

    let output_json = batson_to_json_string(&batson).expect("failed to convert batson to JSON");
    println!("output json length: {}", output_json.len());

    if let Some(path) = std::env::args().nth(2) {
        let path: Vec<BatsonPath> = path.split('.').map(to_batson_path).collect();
        let start = std::time::Instant::now();
        let value = batson::get::get_str(&batson, &path).expect("failed to get value");
        let elapsed = start.elapsed();
        println!("Found value: {value:?} (time taken: {elapsed:?})");
    }

    println!("reloading to check round-trip");
    let json_value = JsonValue::parse(output_json.as_bytes(), false).expect("invalid JSON");
    let batson = encode_from_json(&json_value).expect("failed to construct batson data");
    let output_json2 = batson_to_json_string(&batson).expect("failed to convert batson to JSON");
    println!("JSON unchanged after re-encoding: {:?}", output_json == output_json2);

    println!("\n\noutput json:\n{}", output_json);
}

fn to_batson_path(s: &str) -> BatsonPath {
    if s.chars().all(char::is_numeric) {
        let index: usize = s.parse().unwrap();
        index.into()
    } else {
        s.into()
    }
}
