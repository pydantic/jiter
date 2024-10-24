#![no_main]

use batson::{batson_to_json_string, encode_from_json};
use jiter::JsonValue;

use libfuzzer_sys::fuzz_target;

fn round_trip(json: String) {
    let Ok(jiter_value1) = JsonValue::parse(json.as_bytes(), false) else {
        return;
    };
    let bytes1 = encode_from_json(&jiter_value1).unwrap();
    let json1 = batson_to_json_string(&bytes1).unwrap();

    let jiter_value2 = JsonValue::parse(json1.as_bytes(), false).unwrap();
    let bytes2 = encode_from_json(&jiter_value2).unwrap();
    let json2 = batson_to_json_string(&bytes2).unwrap();

    assert_eq!(json1, json2);
}

fuzz_target!(|json: String| {
    round_trip(json);
});
