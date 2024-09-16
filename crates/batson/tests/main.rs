use std::fs::File;
use std::io::Read;
use std::sync::Arc;

use jiter::JsonValue;

use batson::get::{contains, get_bool, get_int, get_length, get_str};
use batson::{batson_to_json_string, compare_json_values, decode_to_json_value, encode_from_json};

#[test]
fn round_trip_all() {
    let v: JsonValue<'static> = JsonValue::Object(Arc::new(vec![
        // primitives
        ("null".into(), JsonValue::Null),
        ("false".into(), JsonValue::Bool(false)),
        ("true".into(), JsonValue::Bool(true)),
        // int
        ("int-zero".into(), JsonValue::Int(0)),
        ("int-in-header".into(), JsonValue::Int(9)),
        ("int-8".into(), JsonValue::Int(123)),
        ("int-32".into(), JsonValue::Int(1_000)),
        ("int-64".into(), JsonValue::Int(i64::from(i32::MAX) + 5)),
        ("int-max".into(), JsonValue::Int(i64::MAX)),
        ("int-neg-in-header".into(), JsonValue::Int(-9)),
        ("int-neg-8".into(), JsonValue::Int(-123)),
        ("int-neg-32".into(), JsonValue::Int(-1_000)),
        ("int-gex-64".into(), JsonValue::Int(-(i64::from(i32::MAX) + 5))),
        ("int-min".into(), JsonValue::Int(i64::MIN)),
        // floats
        ("float-zero".into(), JsonValue::Float(0.0)),
        ("float-in-header".into(), JsonValue::Float(9.0)),
        ("float-pos".into(), JsonValue::Float(123.45)),
        ("float-pos2".into(), JsonValue::Float(123_456_789.0)),
        ("float-neg".into(), JsonValue::Float(-123.45)),
        ("float-neg2".into(), JsonValue::Float(-123_456_789.0)),
        // strings
        ("str-empty".into(), JsonValue::Str("".into())),
        ("str-short".into(), JsonValue::Str("foo".into())),
        ("str-larger".into(), JsonValue::Str("foo bat spam".into())),
        // het array
        (
            "het-array".into(),
            JsonValue::Array(Arc::new(vec![
                JsonValue::Int(42),
                JsonValue::Str("foobar".into()),
                JsonValue::Bool(true),
            ])),
        ),
        // header array
        (
            "header-array".into(),
            JsonValue::Array(Arc::new(vec![JsonValue::Int(6), JsonValue::Bool(true)])),
        ),
        // i64 array
        (
            "i64-array".into(),
            JsonValue::Array(Arc::new(vec![JsonValue::Int(42), JsonValue::Int(i64::MAX)])),
        ),
        // u8 array
        (
            "u8-array".into(),
            JsonValue::Array(Arc::new(vec![JsonValue::Int(42), JsonValue::Int(255)])),
        ),
    ]));
    let b = encode_from_json(&v).unwrap();

    let v2 = decode_to_json_value(&b).unwrap();
    assert!(compare_json_values(&v2, &v));
}

fn json_to_batson(json: &[u8]) -> Vec<u8> {
    let json_value = JsonValue::parse(json, false).unwrap();
    encode_from_json(&json_value).unwrap()
}

#[test]
fn test_get_bool() {
    let bytes = json_to_batson(br#"{"foo": true}"#);

    assert!(get_bool(&bytes, &["foo".into()]).unwrap().unwrap());
    assert!(get_bool(&bytes, &["bar".into()]).unwrap().is_none());
}

#[test]
fn test_contains() {
    let bytes = json_to_batson(br#"{"foo": true, "bar": [1, 2], "ham": "foo"}"#);

    assert!(contains(&bytes, &["foo".into()]).unwrap());
    assert!(contains(&bytes, &["bar".into()]).unwrap());
    assert!(contains(&bytes, &["ham".into()]).unwrap());
    assert!(contains(&bytes, &["bar".into(), 0.into()]).unwrap());
    assert!(contains(&bytes, &["bar".into(), 1.into()]).unwrap());

    assert!(!contains(&bytes, &["spam".into()]).unwrap());
    assert!(!contains(&bytes, &["bar".into(), 2.into()]).unwrap());
    assert!(!contains(&bytes, &["ham".into(), 0.into()]).unwrap());
}

#[test]
fn test_get_str_object() {
    let bytes = json_to_batson(br#"{"foo": "bar", "spam": true}"#);

    assert_eq!(get_str(&bytes, &["foo".into()]).unwrap().unwrap(), "bar");
    assert!(get_str(&bytes, &["bar".into()]).unwrap().is_none());
    assert!(get_str(&bytes, &["spam".into()]).unwrap().is_none());
}

#[test]
fn test_get_str_array() {
    let bytes = json_to_batson(br#"["foo", 123, "bar"]"#);

    assert_eq!(get_str(&bytes, &[0.into()]).unwrap().unwrap(), "foo");
    assert_eq!(get_str(&bytes, &[2.into()]).unwrap().unwrap(), "bar");

    assert!(get_str(&bytes, &["bar".into()]).unwrap().is_none());
    assert!(get_str(&bytes, &[3.into()]).unwrap().is_none());
}

#[test]
fn test_get_str_nested() {
    let bytes = json_to_batson(br#"{"foo": [null, {"bar": "baz"}]}"#);

    assert_eq!(
        get_str(&bytes, &["foo".into(), 1.into(), "bar".into()])
            .unwrap()
            .unwrap(),
        "baz"
    );

    assert!(get_str(&bytes, &["foo".into()]).unwrap().is_none());
    assert!(get_str(&bytes, &["spam".into(), 1.into()]).unwrap().is_none());
    assert!(get_str(&bytes, &["spam".into(), 1.into(), "bar".into(), 6.into()])
        .unwrap()
        .is_none());
}

#[test]
fn test_get_int_object() {
    let bytes = json_to_batson(br#"{"foo": 42, "spam": true}"#);

    assert_eq!(get_int(&bytes, &["foo".into()]).unwrap().unwrap(), 42);
    assert!(get_int(&bytes, &["bar".into()]).unwrap().is_none());
    assert!(get_int(&bytes, &["spam".into()]).unwrap().is_none());
}

#[test]
fn test_get_int_het_array() {
    let bytes = json_to_batson(br#"[-42, "foo", 922337203685477580]"#);

    assert_eq!(get_int(&bytes, &[0.into()]).unwrap().unwrap(), -42);
    assert_eq!(get_int(&bytes, &[2.into()]).unwrap().unwrap(), 922_337_203_685_477_580);

    assert!(get_int(&bytes, &["bar".into()]).unwrap().is_none());
    assert!(get_int(&bytes, &[3.into()]).unwrap().is_none());
}

#[test]
fn test_get_int_u8_array() {
    let bytes = json_to_batson(br"[42, 123]");

    assert_eq!(get_int(&bytes, &[0.into()]).unwrap().unwrap(), 42);
    assert_eq!(get_int(&bytes, &[1.into()]).unwrap().unwrap(), 123);

    assert!(get_int(&bytes, &["bar".into()]).unwrap().is_none());
    assert!(get_int(&bytes, &[2.into()]).unwrap().is_none());
}

#[test]
fn test_get_int_i64_array() {
    let bytes = json_to_batson(br"[-123, 922337203685477580]");

    assert_eq!(get_int(&bytes, &[0.into()]).unwrap().unwrap(), -123);
    assert_eq!(get_int(&bytes, &[1.into()]).unwrap().unwrap(), 922_337_203_685_477_580);

    assert!(get_int(&bytes, &["bar".into()]).unwrap().is_none());
    assert!(get_int(&bytes, &[2.into()]).unwrap().is_none());
}

#[test]
fn test_get_length() {
    let bytes = json_to_batson(br#"{"foo": [null, {"a": 1, "b": 2}, 1]}"#);

    assert_eq!(get_length(&bytes, &[]).unwrap().unwrap(), 1);
    assert_eq!(get_length(&bytes, &["foo".into()]).unwrap().unwrap(), 3);
    assert_eq!(get_length(&bytes, &["foo".into(), 1.into()]).unwrap().unwrap(), 2);
}

#[test]
fn test_to_json() {
    let bytes = json_to_batson(br" [true, 123] ");
    let s = batson_to_json_string(&bytes).unwrap();
    assert_eq!(s, r"[true,123]");
}

fn json_round_trip(input_json: &str) {
    let bytes = json_to_batson(input_json.as_bytes());
    let output_json = batson_to_json_string(&bytes).unwrap();
    assert_eq!(&output_json, input_json);
}

macro_rules! json_round_trip_tests {
    ($($name:ident => $json:literal;)*) => {
        $(
            paste::item! {
                #[test]
                fn [< json_round_trip_ $name >]() {
                    json_round_trip($json);
                }
            }
        )*
    }
}

json_round_trip_tests!(
    array_empty => r"[]";
    array_bool => r"[true,false]";
    array_bool_int => r"[true,123]";
    array_u8 => r"[1,2,44,255]";
    array_i64 => r"[-1,2,44,255,1234]";
    array_header => r#"[6,true,false,null,0,[],{},""]"#;
    array_het => r#"[true,123,"foo",null]"#;
    string_empty => r#""""#;
    string_hello => r#""hello""#;
    string_escape => r#""\"he\nllo\"""#;
    string_unicode => r#"{"£":"🤪"}"#;
    object_empty => r#"{}"#;
    object_bool => r#"{"foo":true}"#;
    object_two => r#"{"foo":1,"bar":2}"#;
    object_three => r#"{"foo":1,"bar":2,"baz":3}"#;
    object_int => r#"{"foo":123}"#;
    object_string => r#"{"foo":"bar"}"#;
    object_array => r#"{"foo":[1,2]}"#;
    object_nested => r#"{"foo":{"bar":true}}"#;
    object_nested_array => r#"{"foo":{"bar":[1,2]}}"#;
    object_nested_array_nested => r#"{"foo":{"bar":[{"baz":true}]}}"#;
    float_zero => r#"0.0"#;
    float_neg => r#"-123.45"#;
    float_pos => r#"123.456789"#;
);

#[test]
fn batson_file() {
    // check the binary format doesn't change
    let json = r#"
        {
            "header_only": [6, true, false, null, 0, [], {}, ""],
            "u8_array": [0, 1, 2, 42, 255],
            "i64_array": [-1, 2, 44, 255, 1234, 922337203685477],
            "het_array": [true, 123, "foo", "£100", null],
            "true": true,
            "false": false,
            "null": null
        }
    "#;
    let bytes = json_to_batson(json.as_bytes());

    let s = batson_to_json_string(&bytes).unwrap();
    assert_eq!(s, json.replace(" ", "").replace("\n", ""));

    let file_path = "tests/batson_example.bin";
    // std::fs::write("tests/batson_example.bin", &bytes).unwrap();

    // read the file and compare
    let mut file = File::open(file_path).unwrap();
    let mut contents = Vec::new();
    file.read_to_end(&mut contents).unwrap();

    assert_eq!(contents, bytes);
    // dbg!(contents.len());
    // dbg!(json.replace(" ", "").replace("\n", "").len());
}
