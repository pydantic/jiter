use indexmap::indexmap;
use std::fs::File;
use std::io::Read;

use donervan::{Chunk, ChunkInfo, Chunker, Decoder, JsonError, JsonResult, JsonValue};

macro_rules! single_expect_ok_or_error {
    ($name:ident, ok, $json:literal, $expected:expr) => {
        paste::item! {
            #[test]
            fn [< single_chunk_ok_ $name >]() {
                let chunks: Vec<ChunkInfo> = Chunker::new($json.as_bytes()).collect::<JsonResult<_>>().unwrap();
                let chunks_str = chunks.iter().map(|c| c.to_string()).collect::<Vec<String>>().join(", ");
                assert_eq!(chunks_str, $expected);
            }
        }
    };
    ($name:ident, err, $json:literal, $error:expr) => {
        paste::item! {
            #[test]
            fn [< single_chunk_xerror_ $name _ $error:snake _error >]() {
                let result: JsonResult<Vec<ChunkInfo>> = Chunker::new($json.as_bytes()).collect();
                match result {
                    Ok(t) => panic!("unexpectedly valid: {:?} -> {:?}", $json, t),
                    Err(e) => assert_eq!(e.error_type, JsonError::$error),
                }
            }
        }
    };
}

macro_rules! single_tests {
    ($($name:ident: $ok_or_err:ident => $input:literal, $expected:expr;)*) => {
        $(
            single_expect_ok_or_error!($name, $ok_or_err, $input, $expected);
        )*
    }
}

single_tests! {
    string: ok => r#""foobar""#, "String(1..7) @ 1:1";
    int_pos: ok => "1234", "+Int(0..4) @ 1:1";
    int_neg: ok => "-1234", "-Int(1..5) @ 1:1";
    int_exp: ok => "20e10", "+Int(0..2e+3..5) @ 1:1";
    float_pos: ok => "12.34", "+Float(0..2.3..5) @ 1:3";
    float_neg: ok => "-12.34", "-Float(1..3.4..6) @ 1:4";
    float_exp: ok => "2.2e10", "+Float(0..1.2..3e+4..6) @ 1:2";
    null: ok => "null", "null @ 1:1";
    v_true: ok => "true", "true @ 1:1";
    v_false: ok => "false", "false @ 1:1";
    offset_true: ok => "  true", "true @ 1:3";
    string_unclosed: err => r#""foobar"#, UnexpectedEnd;
    bad_int: err => "-", InvalidNumber;
    bad_true: err => "truX", InvalidTrue;
    bad_true: err => "tru", UnexpectedEnd;
    bad_false: err => "falsX", InvalidFalse;
    bad_false: err => "fals", UnexpectedEnd;
    bad_null: err => "nulX", InvalidNull;
    bad_null: err => "nul", UnexpectedEnd;
    object_trailing_comma: err => r#"{"foo": "bar",}"#, UnexpectedCharacter;
    array_trailing_comma: err => r#"[1, 2,]"#, UnexpectedCharacter;
    array_bool: ok => "[true, false]", "[ @ 1:1, true @ 1:2, false @ 1:8, ] @ 1:13";
    object_string: ok => r#"{"foo": "ba"}"#, "{ @ 1:1, Key(2..5) @ 1:2, String(9..11) @ 1:9, } @ 1:13";
    object_null: ok => r#"{"foo": null}"#, "{ @ 1:1, Key(2..5) @ 1:2, null @ 1:9, } @ 1:13";
    object_bool_compact: ok => r#"{"foo":true}"#, "{ @ 1:1, Key(2..5) @ 1:2, true @ 1:8, } @ 1:12";
    deep_array: ok => r#"[["Not too deep"]]"#, "[ @ 1:1, [ @ 1:2, String(3..15) @ 1:3, ] @ 1:17, ] @ 1:18";
    object_key_int: err => r#"{4: 4}"#, UnexpectedCharacter;
    array_no_close: err => r#"["#, UnexpectedEnd;
    array_double_close: err => r#"[1]]"#, UnexpectedCharacter;
}

#[test]
fn invalid_string_controls() {
    let json = "\"123\x08\x0c\n\r\t\"";
    let result: JsonResult<Vec<ChunkInfo>> = Chunker::new(json.as_bytes()).collect();
    match result {
        Ok(t) => panic!("unexpectedly valid: {:?} -> {:?}", json, t),
        Err(e) => assert_eq!(e.error_type, JsonError::InvalidString(3)),
    }
}

#[test]
fn parse_str() {
    let json = "foobar";
    let result_string = Decoder::new(json.as_bytes()).decode_string(0..3, (0, 0)).unwrap();
    assert_eq!(result_string, "foo".to_string());
}

#[test]
fn json_parse_str() {
    let json = r#" "foobar" "#;
    let data = json.as_bytes();
    let chunks: Vec<ChunkInfo> = Chunker::new(data).collect::<JsonResult<_>>().unwrap();
    assert_eq!(chunks.len(), 1);
    let first_chunk = chunks[0].clone();
    let debug = format!("{}", first_chunk);
    assert_eq!(debug, "String(2..8) @ 1:2");

    let range = match first_chunk.chunk_type {
        Chunk::String(range) => range,
        _ => unreachable!(),
    };
    let result_string = Decoder::new(data).decode_string(range, (0, 0)).unwrap();
    assert_eq!(result_string, "foobar");
}

macro_rules! string_tests {
    ($($name:ident: $json:literal => $expected:expr;)*) => {
        $(
            paste::item! {
                #[test]
                fn [< string_parsing_ $name >]() {
                    let data = $json.as_bytes();
                    let chunks: Vec<ChunkInfo> = Chunker::new(data).collect::<JsonResult<_>>().unwrap();
                    assert_eq!(chunks.len(), 1);
                    let first_chunk = chunks[0].clone();
                    let range = match first_chunk.chunk_type {
                        Chunk::String(range) => range,
                        v => panic!("expected string, not {:?}", v),
                    };
                    let result_string = Decoder::new(data).decode_string(range, (0, 0)).unwrap();
                    assert_eq!(result_string, $expected);
                }
            }
        )*
    }
}

string_tests! {
    simple: r#""foobar""# => "foobar";
    newline: "\"foo\\nbar\"" => "foo\nbar";
    pound_sign: "\"\\u00a3\"" => "£";
    double_quote: r#""\"""# => r#"""#;
    backslash: r#""\\""# => r#"\"#;
    controls: "\"\\b\\f\\n\\r\\t\"" => "\u{8}\u{c}\n\r\t";
    controls_python: "\"\\b\\f\\n\\r\\t\"" => "\x08\x0c\n\r\t";  // python notation for the same thing
}

#[test]
fn parse_int() {
    for input_value in -1000i64..1000 {
        let json = format!(" {} ", input_value);
        let data = json.as_bytes();
        let chunks: Vec<ChunkInfo> = Chunker::new(data).collect::<JsonResult<_>>().unwrap();
        assert_eq!(chunks.len(), 1);
        let first_chunk = chunks[0].clone();
        let (positive, range) = match first_chunk.chunk_type {
            Chunk::Int {
                positive,
                range,
                exponent,
            } => {
                assert_eq!(exponent, None);
                (positive, range)
            }
            v => panic!("expected int, not {:?}", v),
        };
        let result_int = Decoder::new(data).decode_int(positive, range, None, (0, 0)).unwrap();
        assert_eq!(result_int, input_value);
    }
}

#[test]
fn parse_float() {
    for i in -1000..1000 {
        let input_value = i as f64 * 0.1;
        let json = format!("{:.4}", input_value);
        let data = json.as_bytes();
        let chunks: Vec<ChunkInfo> = Chunker::new(data).collect::<JsonResult<_>>().unwrap();
        let first_chunk = chunks[0].clone();
        let (positive, int_range, decimal_range) = match first_chunk.clone().chunk_type {
            Chunk::Float {
                positive,
                int_range,
                decimal_range,
                exponent,
            } => {
                assert_eq!(exponent, None);
                (positive, int_range, decimal_range)
            }
            v => panic!("expected float, not {:?} (json: {:?}", v, json),
        };
        let result_int = Decoder::new(data)
            .decode_float(positive, int_range, decimal_range, None, (0, 0))
            .unwrap();
        assert!((result_int - input_value).abs() < 1e-6);
    }
}

#[test]
fn parse_value() {
    let json = r#"{"foo": "bar", "spam": [1, null, true]}"#;
    let v = JsonValue::parse(json.as_bytes()).unwrap();
    assert_eq!(
        v,
        JsonValue::Object(indexmap! {
            "foo".to_string() => JsonValue::String("bar".to_string()),
            "spam".to_string() => JsonValue::Array(
                vec![
                    JsonValue::Int(1),
                    JsonValue::Null,
                    JsonValue::Bool(true),
                ],
            ),
        },)
    );
}

#[test]
fn parse_value_nested() {
    let json = r#"[1, 2, [3, 4], 5, 6]"#;
    let v = JsonValue::parse(json.as_bytes()).unwrap();
    assert_eq!(
        v,
        JsonValue::Array(vec![
            JsonValue::Int(1),
            JsonValue::Int(2),
            JsonValue::Array(vec![JsonValue::Int(3), JsonValue::Int(4)]),
            JsonValue::Int(5),
            JsonValue::Int(6),
        ],)
    )
}

fn read_file(path: &str) -> String {
    let mut file = File::open(path).unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();
    contents
}

#[test]
fn pass1_to_value() {
    let json = read_file("./benches/pass1.json");
    let json_data = json.as_bytes();
    let v = JsonValue::parse(json_data).unwrap();
    let array = match v {
        JsonValue::Array(array) => array,
        v => panic!("expected array, not {:?}", v),
    };
    assert_eq!(array.len(), 20);
    assert_eq!(array[0], JsonValue::String("JSON Test Pattern pass1".to_string()));
}
