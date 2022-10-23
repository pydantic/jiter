use indexmap::indexmap;

use donervan::parse::{parse_float, parse_int, parse_string};
use donervan::{Chunk, ChunkInfo, Chunker, JsonError, JsonResult, JsonValue};

macro_rules! single_expect_ok_or_error {
    ($name:ident, ok, $json:literal, $expected:expr) => {
        paste::item! {
            #[test]
            fn [< single_chunk_ok_ $name >]() {
                let chunks: Vec<ChunkInfo> = Chunker::new($json.as_bytes()).collect::<JsonResult<_>>().unwrap();
                assert_eq!(chunks.len(), 1);
                let first_chunk = chunks[0].clone();
                assert_eq!(format!("{:?}", first_chunk), $expected);
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
    string: ok => r#""foobar""#, "ChunkInfo { key: None, chunk_type: String(1..7), loc: (1, 1) }";
    int_neg: ok => "-1234", "ChunkInfo { key: None, chunk_type: Int { positive: false, range: 1..5, exponent: None }, loc: (1, 1) }";
    int_pos: ok => "1234", "ChunkInfo { key: None, chunk_type: Int { positive: true, range: 0..4, exponent: None }, loc: (1, 1) }";
    int_exp: ok => "20e10", "ChunkInfo { key: None, chunk_type: Int { positive: true, range: 0..2, exponent: Some(Exponent { positive: true, range: 3..5 }) }, loc: (1, 1) }";
    float_pos: ok => "12.34", "ChunkInfo { key: None, chunk_type: Float { positive: true, int_range: 0..2, decimal_range: 3..5, exponent: None }, loc: (1, 1) }";
    float_neg: ok => "-12.34", "ChunkInfo { key: None, chunk_type: Float { positive: false, int_range: 1..3, decimal_range: 4..6, exponent: None }, loc: (1, 1) }";
    float_exp: ok => "2.2e10", "ChunkInfo { key: None, chunk_type: Float { positive: true, int_range: 0..1, decimal_range: 2..3, exponent: Some(Exponent { positive: true, range: 4..6 }) }, loc: (1, 1) }";
    null: ok => "null", "ChunkInfo { key: None, chunk_type: Null, loc: (1, 1) }";
    v_true: ok => "true", "ChunkInfo { key: None, chunk_type: True, loc: (1, 1) }";
    v_false: ok => "false", "ChunkInfo { key: None, chunk_type: False, loc: (1, 1) }";
    offset_true: ok => "  true", "ChunkInfo { key: None, chunk_type: True, loc: (1, 3) }";
    string_unclosed: err => r#""foobar"#, UnexpectedEnd;
    bad_int: err => "-", InvalidNumber;
    bad_true: err => "truX", InvalidTrue;
    bad_true: err => "tru", UnexpectedEnd;
    bad_false: err => "falsX", InvalidFalse;
    bad_false: err => "fals", UnexpectedEnd;
    bad_null: err => "nulX", InvalidNull;
    bad_null: err => "nul", UnexpectedEnd;
    object_trailing_comma: err => r#"{"foo": "bar",}"#, ExpectingKey;
    array_trailing_comma: err => r#"[1, 2,]"#, UnexpectedCharacter;
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
fn chunk_array() {
    let json = "[true, false]";
    let chunks: Vec<ChunkInfo> = Chunker::new(json.as_bytes()).collect::<JsonResult<_>>().unwrap();
    assert_eq!(
        chunks,
        vec![
            ChunkInfo {
                key: None,
                chunk_type: Chunk::ArrayStart,
                loc: (1, 1),
            },
            ChunkInfo {
                key: None,
                chunk_type: Chunk::True,
                loc: (1, 2),
            },
            ChunkInfo {
                key: None,
                chunk_type: Chunk::False,
                loc: (1, 6),
            },
            ChunkInfo {
                key: None,
                chunk_type: Chunk::ArrayEnd,
                loc: (1, 13),
            },
        ]
    );
}

#[test]
fn chunk_object() {
    let json = r#"{"foobar": null}"#;
    let chunks: Vec<ChunkInfo> = Chunker::new(json.as_bytes()).collect::<JsonResult<_>>().unwrap();
    assert_eq!(
        chunks,
        vec![
            ChunkInfo {
                key: None,
                chunk_type: Chunk::ObjectStart,
                loc: (1, 1),
            },
            ChunkInfo {
                key: Some(2..8,),
                chunk_type: Chunk::Null,
                loc: (1, 2),
            },
            ChunkInfo {
                key: None,
                chunk_type: Chunk::ObjectEnd,
                loc: (1, 16),
            },
        ]
    );
}

#[test]
fn test_parse_str() {
    let json = "foobar";
    let result_string = parse_string(json.as_bytes(), 0..3).unwrap();
    assert_eq!(result_string, "foo".to_string());
}

#[test]
fn test_json_parse_str() {
    let json = r#" "foobar" "#;
    let data = json.as_bytes();
    let chunks: Vec<ChunkInfo> = Chunker::new(data).collect::<JsonResult<_>>().unwrap();
    assert_eq!(chunks.len(), 1);
    let first_chunk = chunks[0].clone();
    let debug = format!("{:?}", first_chunk);
    assert_eq!(debug, "ChunkInfo { key: None, chunk_type: String(2..8), loc: (1, 2) }");

    let range = match first_chunk.chunk_type {
        Chunk::String(range) => range,
        _ => unreachable!(),
    };
    let result_string = parse_string(data, range).unwrap();
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
                    let result_string = parse_string(data, range).unwrap();
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
fn test_parse_int() {
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
        let result_int = parse_int(data, positive, range).unwrap();
        assert_eq!(result_int, input_value);
    }
}

#[test]
fn test_parse_float() {
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
        let result_int = parse_float(data, positive, int_range, decimal_range).unwrap();
        assert!((result_int - input_value).abs() < 1e-6);
    }
}

#[test]
fn test_parse_value() {
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
