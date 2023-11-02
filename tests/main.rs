use std::fs::File;
use std::io::Read;
use std::str::FromStr;
use std::sync::Arc;

use num_bigint::BigInt;
use smallvec::smallvec;

use jiter::{
    FilePosition, Jiter, JiterErrorType, JsonErrorType, JsonResult, JsonType, JsonValue, LazyIndexMap, NumberAny,
    NumberInt, Parser, Peak, StringDecoder, StringDecoderRange,
};

fn json_vec(parser: &mut Parser, peak: Option<Peak>) -> JsonResult<Vec<String>> {
    let mut v = Vec::new();
    let mut tape: Vec<u8> = Vec::new();
    let peak = match peak {
        Some(peak) => peak,
        None => parser.peak()?,
    };

    let position = parser.current_position().short();
    match peak {
        Peak::True => {
            parser.consume_true()?;
            v.push(format!("true @ {position}"));
        }
        Peak::False => {
            parser.consume_false()?;
            v.push(format!("false @ {position}"));
        }
        Peak::Null => {
            parser.consume_null()?;
            v.push(format!("null @ {position}"));
        }
        Peak::String => {
            let range = parser.consume_string::<StringDecoderRange>(&mut tape)?;
            v.push(format!("String({range:?}) @ {position}"));
        }
        Peak::Num(first) => {
            let s = display_number(first, parser)?;
            v.push(s);
        }
        Peak::Array => {
            v.push(format!("[ @ {position}"));
            if let Some(peak) = parser.array_first()? {
                let el_vec = json_vec(parser, Some(peak))?;
                v.extend(el_vec);
                while let Some(peak) = parser.array_step()? {
                    let el_vec = json_vec(parser, Some(peak))?;
                    v.extend(el_vec);
                }
            }
            v.push("]".to_string());
        }
        Peak::Object => {
            v.push(format!("{{ @ {position}"));
            if let Some(key) = parser.object_first::<StringDecoderRange>(&mut tape)? {
                v.push(format!("Key({key:?})"));
                let value_vec = json_vec(parser, None)?;
                v.extend(value_vec);
                while let Some(key) = parser.object_step::<StringDecoderRange>(&mut tape)? {
                    v.push(format!("Key({key:?}"));
                    let value_vec = json_vec(parser, None)?;
                    v.extend(value_vec);
                }
            }
            v.push("}".to_string());
        }
    };
    Ok(v)
}

fn display_number(first: u8, parser: &mut Parser) -> JsonResult<String> {
    let position = parser.current_position().short();
    let number = parser.consume_number::<NumberAny>(first)?;
    let s = match number {
        NumberAny::Int(NumberInt::Int(int)) => {
            format!("Int({int}) @ {position}")
        }
        NumberAny::Int(NumberInt::BigInt(big_int)) => {
            format!("BigInt({big_int}) @ {position}")
        }
        NumberAny::Float(float) => {
            format!("Float({float}) @ {position}")
        }
    };
    Ok(s)
}

macro_rules! single_expect_ok_or_error {
    ($name:ident, ok, $json:literal, $expected:expr) => {
        paste::item! {
            #[allow(non_snake_case)]
            #[test]
            fn [< single_element_ok__ $name >]() {
                let mut parser = Parser::new($json.as_bytes());
                let elements = json_vec(&mut parser, None).unwrap().join(", ");
                assert_eq!(elements, $expected);
                parser.finish().unwrap();
            }
        }
    };

    ($name:ident, err, $json:literal, $expected_error:literal) => {
        paste::item! {
            #[allow(non_snake_case)]
            #[test]
            fn [< single_element_xerror__ $name >]() {
                let mut parser = Parser::new($json.as_bytes());
                let result = json_vec(&mut parser, None);
                let first_value = match result {
                    Ok(v) => v,
                    Err(e) => {
                        let position = parser.error_position(e.index);
                        let actual_error = format!("{:?} @ {}", e.error_type, position.short());
                        assert_eq!(actual_error, $expected_error);

                        let full_error = format!("{} at {}", e.error_type, position);
                        let serde_err = serde_json::from_str::<serde_json::Value>($json).unwrap_err();
                        assert_eq!(full_error, serde_err.to_string());
                        return
                    },
                };
                let result = parser.finish();
                match result {
                    Ok(_) => panic!("unexpectedly valid at finish: {:?} -> {:?}", $json, first_value),
                    Err(e) => {
                        // to check to_string works, and for coverage
                        e.to_string();
                        let position = parser.error_position(e.index);
                        let actual_error = format!("{:?} @ {}", e.error_type, position.short());
                        assert_eq!(actual_error, $expected_error);
                        return
                    },
                }
            }
        }
    };
}

macro_rules! single_tests {
    ($($name:ident: $ok_or_err:ident => $input:literal, $expected:literal;)*) => {
        $(
            single_expect_ok_or_error!($name, $ok_or_err, $input, $expected);
        )*
    }
}

single_tests! {
    string: ok => r#""foobar""#, "String(1..7) @ 1:1";
    int_pos: ok => "1234", "Int(1234) @ 1:1";
    int_neg: ok => "-1234", "Int(-1234) @ 1:1";
    big_int: ok => "92233720368547758070", "BigInt(92233720368547758070) @ 1:1";
    big_int_neg: ok => "-92233720368547758070", "BigInt(-92233720368547758070) @ 1:1";
    big_int2: ok => "99999999999999999999999999999999999999999999999999", "BigInt(99999999999999999999999999999999999999999999999999) @ 1:1";
    float_pos: ok => "12.34", "Float(12.34) @ 1:1";
    float_neg: ok => "-12.34", "Float(-12.34) @ 1:1";
    float_exp: ok => "2.2e10", "Float(22000000000) @ 1:1";
    float_simple_exp: ok => "20e10", "Float(200000000000) @ 1:1";
    float_exp_pos: ok => "2.2e+10", "Float(22000000000) @ 1:1";
    float_exp_neg: ok => "2.2e-2", "Float(0.022) @ 1:1";
    float_exp_zero: ok => "0.000e123", "Float(0) @ 1:1";
    float_exp_massive1: ok => "2e2147483647", "Float(inf) @ 1:1";
    float_exp_massive2: ok => "2e2147483648", "Float(inf) @ 1:1";
    float_exp_massive3: ok => "2e2147483646", "Float(inf) @ 1:1";
    float_exp_massive4: ok => "2e2147483646", "Float(inf) @ 1:1";
    float_exp_massive5: ok => "18446744073709551615000.0", "Float(18446744073709552000000) @ 1:1";
    float_exp_massive6: ok => "0.0E667", "Float(0) @ 1:1";
    float_exp_tiny0: ok => "2e-2147483647", "Float(0) @ 1:1";
    float_exp_tiny1: ok => "2e-2147483648", "Float(0) @ 1:1";
    float_exp_tiny2: ok => "2e-2147483646", "Float(0) @ 1:1";
    float_exp_tiny3: ok => "8e-7766666666", "Float(0) @ 1:1";
    float_exp_tiny4: ok => "200.08e-76200000102", "Float(0) @ 1:1";
    float_exp_tiny5: ok => "0e459", "Float(0) @ 1:1";
    null: ok => "null", "null @ 1:1";
    v_true: ok => "true", "true @ 1:1";
    v_false: ok => "false", "false @ 1:1";
    offset_true: ok => "  true", "true @ 1:3";
    empty: err => "", "EofWhileParsingValue @ 1:0";
    string_unclosed: err => r#""foobar"#, "EofWhileParsingString @ 1:7";
    bad_int_neg: err => "-", "EofWhileParsingValue @ 1:1";
    bad_true1: err => "truX", "ExpectedSomeIdent @ 1:4";
    bad_true2: err => "tru", "EofWhileParsingValue @ 1:3";
    bad_true3: err => "trX", "ExpectedSomeIdent @ 1:3";
    bad_false1: err => "falsX", "ExpectedSomeIdent @ 1:5";
    bad_false2: err => "fals", "EofWhileParsingValue @ 1:4";
    bad_null1: err => "nulX", "ExpectedSomeIdent @ 1:4";
    bad_null2: err => "nul", "EofWhileParsingValue @ 1:3";
    object_trailing_comma: err => r#"{"foo": "bar",}"#, "TrailingComma @ 1:15";
    array_trailing_comma: err => r#"[1, 2,]"#, "TrailingComma @ 1:7";
    array_wrong_char_after_comma: err => r#"[1, 2,;"#, "ExpectedSomeValue @ 1:7";
    array_end_after_comma: err => "[9,", "EofWhileParsingValue @ 1:3";
    object_wrong_char: err => r#"{"foo":42;"#, "ExpectedObjectCommaOrEnd @ 1:10";
    object_wrong_char_after_comma: err => r#"{"foo":42,;"#, "KeyMustBeAString @ 1:11";
    object_end_after_comma: err => r#"{"x": 9,"#, "EofWhileParsingValue @ 1:8";
    object_end_after_colon: err => r#"{"":"#, "EofWhileParsingValue @ 1:4";
    eof_while_parsing_object: err => r#"{"foo": 1"#, "EofWhileParsingObject @ 1:9";
    expected_colon: err => r#"{"foo"1"#, "ExpectedColon @ 1:7";
    array_bool: ok => "[true, false]", "[ @ 1:1, true @ 1:2, false @ 1:8, ]";
    object_string: ok => r#"{"foo": "ba"}"#, "{ @ 1:1, Key(2..5), String(9..11) @ 1:9, }";
    object_null: ok => r#"{"foo": null}"#, "{ @ 1:1, Key(2..5), null @ 1:9, }";
    object_bool_compact: ok => r#"{"foo":true}"#, "{ @ 1:1, Key(2..5), true @ 1:8, }";
    deep_array: ok => r#"[["Not too deep"]]"#, "[ @ 1:1, [ @ 1:2, String(3..15) @ 1:3, ], ]";
    object_key_int: err => r#"{4: 4}"#, "KeyMustBeAString @ 1:2";
    array_no_close: err => r#"["#, "EofWhileParsingList @ 1:1";
    array_double_close: err => "[1]]", "TrailingCharacters @ 1:4";
    invalid_float_e_end: err => "0E", "EofWhileParsingValue @ 1:2";
    invalid_float_dot_end: err => "0.", "EofWhileParsingValue @ 1:2";
    invalid_float_bracket: err => "2E[", "InvalidNumber @ 1:3";
    trailing_char: err => "2z", "TrailingCharacters @ 1:2";
    invalid_number_newline: err => "-\n", "InvalidNumber @ 2:0";
    double_zero: err => "00", "InvalidNumber @ 1:2";
    double_zero_one: err => "001", "InvalidNumber @ 1:2";
    first_line: err => "[1 x]", "ExpectedListCommaOrEnd @ 1:4";
    second_line: err => "[1\nx]", "ExpectedListCommaOrEnd @ 2:1";
    floats_error: err => "06", "InvalidNumber @ 1:2";
    unexpect_value: err => "[\u{16}\u{8}", "ExpectedSomeValue @ 1:2";
}

#[test]
fn invalid_string_controls() {
    let json = "\"123\x08\x0c\n\r\t\"";
    let mut tape: Vec<u8> = Vec::new();
    let b = json.as_bytes();
    let mut parser = Parser::new(b);
    let peak = parser.peak().unwrap();
    assert!(matches!(peak, Peak::String));
    let e = parser.consume_string::<StringDecoder>(&mut tape).unwrap_err();
    assert_eq!(e.error_type, JsonErrorType::ControlCharacterWhileParsingString);
    assert_eq!(e.index, 4);
    assert_eq!(parser.error_position(e.index), FilePosition::new(1, 5));
    assert_eq!(
        e.to_string(),
        "control character (\\u0000-\\u001F) found while parsing a string at index 4"
    );
}

#[test]
fn json_parse_str() {
    let json = r#" "foobar" "#;
    let mut tape: Vec<u8> = Vec::new();
    let data = json.as_bytes();
    let mut parser = Parser::new(data);
    let peak = parser.peak().unwrap();
    assert_eq!(peak, Peak::String);
    assert_eq!(parser.index, 1);
    assert_eq!(parser.current_position(), FilePosition::new(1, 2));

    let result_string = parser.consume_string::<StringDecoder>(&mut tape).unwrap();
    assert_eq!(result_string.as_str(), "foobar");
    parser.finish().unwrap();
}

macro_rules! string_tests {
    ($($name:ident: $json:literal => $expected:expr;)*) => {
        $(
            paste::item! {
                #[test]
                fn [< string_parsing_ $name >]() {
                    let data = $json.as_bytes();
                    let mut tape: Vec<u8> = Vec::new();
                    let mut parser = Parser::new(data);
                    let peak = parser.peak().unwrap();
                    assert!(matches!(peak, Peak::String));
                    let strs = parser.consume_string::<StringDecoder>(&mut tape).unwrap();
                    assert_eq!(strs.as_str(), $expected);
                    parser.finish().unwrap();
                }
            }
        )*
    }
}

string_tests! {
    simple: r#"  "foobar"  "# => "foobar";
    newline: r#"  "foo\nbar"  "# => "foo\nbar";
    pound_sign: r#"  "\u00a3"  "# => "£";
    double_quote: r#"  "\""  "# => r#"""#;
    backslash: r#""\\""# => r"\";
    controls: "\"\\b\\f\\n\\r\\t\"" => "\u{8}\u{c}\n\r\t";
    controls_python: "\"\\b\\f\\n\\r\\t\"" => "\x08\x0c\n\r\t";  // python notation for the same thing
}

macro_rules! string_test_errors {
    ($($name:ident: $json:literal => $expected_error:literal;)*) => {
        $(
            paste::item! {
                #[test]
                fn [< string_parsing_errors_ $name >]() {
                    let data = $json.as_bytes();
                    let mut tape: Vec<u8> = Vec::new();
                    let mut parser = Parser::new(data);
                    let peak = parser.peak().unwrap();
                    assert_eq!(peak, Peak::String);
                    match parser.consume_string::<StringDecoder>(&mut tape) {
                        Ok(t) => panic!("unexpectedly valid: {:?} -> {:?}", $json, t),
                        Err(e) => {
                            // to check to_string works, and for coverage
                            e.to_string();
                            let position = parser.error_position(e.index);
                            let actual_error = format!("{:?} @ {} - {}", e.error_type, e.index, position.short());
                            assert_eq!(actual_error, $expected_error);
                        }
                    }
                }
            }
        )*
    }
}

string_test_errors! {
    u4_unclosed: r#""\uxx"# => "EofWhileParsingString @ 5 - 1:5";
    u4_unclosed2: r#""\udBdd"# => "EofWhileParsingString @ 7 - 1:7";
    line_leading_surrogate: r#""\uddBd""# => "LoneLeadingSurrogateInHexEscape @ 6 - 1:7";
    unexpected_hex_escape1: r#""\udBd8x"# => "UnexpectedEndOfHexEscape @ 7 - 1:8";
    unexpected_hex_escape2: r#""\udBd8xx"# => "UnexpectedEndOfHexEscape @ 7 - 1:8";
    unexpected_hex_escape3: "\"un\\uDBBB\0" => "UnexpectedEndOfHexEscape @ 9 - 1:10";
    unexpected_hex_escape4: r#""\ud8e0\e"# => "UnexpectedEndOfHexEscape @ 8 - 1:9";
    newline_in_string: "\"\n" => "ControlCharacterWhileParsingString @ 1 - 2:0";
    invalid_escape: r#" "\u12xx" "# => "InvalidEscape @ 6 - 1:7";
}

#[test]
fn invalid_unicode_code() {
    let json = vec![34, 92, 34, 206, 44, 163, 34];
    // dbg!(json.iter().map(|b| *b as char).collect::<Vec<_>>());
    let mut tape: Vec<u8> = Vec::new();
    let mut parser = Parser::new(&json);
    let p = parser.peak().unwrap();
    assert!(matches!(p, Peak::String));
    let e = parser.consume_string::<StringDecoder>(&mut tape).unwrap_err();
    assert_eq!(e.error_type, JsonErrorType::InvalidUnicodeCodePoint);
    assert_eq!(e.index, 3);
    assert_eq!(parser.error_position(e.index), FilePosition::new(1, 4));
}

#[test]
fn test_key_str() {
    let json = r#"{"foo": "bar"}"#;
    let mut tape: Vec<u8> = Vec::new();
    let mut parser = Parser::new(json.as_bytes());
    let p = parser.peak().unwrap();
    assert!(matches!(p, Peak::Object));
    let k = parser.object_first::<StringDecoder>(&mut tape).unwrap().unwrap();
    assert_eq!(k.as_str(), "foo");
    let p = parser.peak().unwrap();
    assert!(matches!(p, Peak::String));
    let v = parser.consume_string::<StringDecoder>(&mut tape).unwrap();
    assert_eq!(v.to_string(), "bar".to_string());
    assert_eq!(v.as_str(), "bar");
    let next_key = parser.object_step::<StringDecoder>(&mut tape).unwrap();
    assert!(next_key.is_none());
    parser.finish().unwrap();
}

#[test]
fn test_key_bytes() {
    let json = r#"{"foo": "bar"}"#.as_bytes();
    let mut tape: Vec<u8> = Vec::new();
    let mut parser = Parser::new(json);
    let p = parser.peak().unwrap();
    assert!(matches!(p, Peak::Object));
    let k = parser.object_first::<StringDecoderRange>(&mut tape).unwrap().unwrap();
    assert_eq!(json[k], *b"foo");
    let p = parser.peak().unwrap();
    assert!(matches!(p, Peak::String));
    let v = parser.consume_string::<StringDecoderRange>(&mut tape).unwrap();
    assert_eq!(json[v], *b"bar");
    let next_key = parser.object_step::<StringDecoder>(&mut tape).unwrap();
    assert!(next_key.is_none());
    parser.finish().unwrap();
}

macro_rules! test_position {
    ($($name:ident: $data:literal, $find:literal, $expected:expr;)*) => {
        $(
            paste::item! {
                #[test]
                fn [< test_position_ $name >]() {
                    assert_eq!(FilePosition::find($data, $find), $expected);
                }
            }
        )*
    }
}

test_position! {
    empty_zero: b"", 0, FilePosition::new(1, 0);
    empty_one: b"", 1, FilePosition::new(1, 0);
    first_line_zero: b"123456", 0, FilePosition::new(1, 1);
    first_line_first: b"123456", 1, FilePosition::new(1, 2);
    first_line_3rd: b"123456", 3, FilePosition::new(1, 4);
    first_line_5th: b"123456", 5, FilePosition::new(1, 6);
    first_line_last: b"123456", 6, FilePosition::new(1, 6);
    first_line_after: b"123456", 7, FilePosition::new(1, 6);
    second_line0: b"123456\n789", 6, FilePosition::new(2, 0);
    second_line1: b"123456\n789", 7, FilePosition::new(2, 1);
    second_line2: b"123456\n789", 8, FilePosition::new(2, 2);
}

#[test]
fn parse_tiny_float() {
    let v = JsonValue::parse(b"8e-7766666666").unwrap();
    assert_eq!(v, JsonValue::Float(0.0));
}

#[test]
fn parse_zero_float() {
    let v = JsonValue::parse(b"0.1234").unwrap();
    match v {
        JsonValue::Float(v) => assert!((0.1234 - v).abs() < 1e-6),
        other => panic!("unexpected value: {other:?}"),
    };
}

#[test]
fn parse_zero_exp_float() {
    let v = JsonValue::parse(b"0.12e3").unwrap();
    match v {
        JsonValue::Float(v) => assert!((120.0 - v).abs() < 1e-3),
        other => panic!("unexpected value: {other:?}"),
    };
}

#[test]
fn bad_lower_value_in_string() {
    let bytes: Vec<u8> = vec![34, 27, 32, 34];
    let e = JsonValue::parse(&bytes).unwrap_err();
    assert_eq!(e.error_type, JsonErrorType::ControlCharacterWhileParsingString);
    assert_eq!(e.index, 1);
    assert_eq!(e.position, FilePosition::new(1, 2));
}

#[test]
fn bad_high_order_string() {
    let bytes: Vec<u8> = vec![34, 32, 32, 210, 34];
    let e = JsonValue::parse(&bytes).unwrap_err();
    assert_eq!(e.error_type, JsonErrorType::InvalidUnicodeCodePoint);
    assert_eq!(e.index, 4);
    assert_eq!(e.to_string(), "invalid unicode code point at line 1 column 5")
}

#[test]
fn udb_string() {
    let bytes: Vec<u8> = vec![34, 92, 117, 100, 66, 100, 100, 92, 117, 100, 70, 100, 100, 34];
    let v = JsonValue::parse(&bytes).unwrap();
    match v {
        JsonValue::Str(s) => assert_eq!(s.as_bytes(), [244, 135, 159, 157]),
        _ => panic!("unexpected value {v:?}"),
    }
}

#[test]
fn json_value_object() {
    let json = r#"{"foo": "bar", "spam": [1, null, true]}"#;
    let v = JsonValue::parse(json.as_bytes()).unwrap();

    let mut expected = LazyIndexMap::new();
    expected.insert("foo".to_string(), JsonValue::Str("bar".to_string()));
    expected.insert(
        "spam".to_string(),
        JsonValue::Array(Arc::new(smallvec![
            JsonValue::Int(1),
            JsonValue::Null,
            JsonValue::Bool(true)
        ])),
    );
    assert_eq!(v, JsonValue::Object(Arc::new(expected)));
}

#[test]
fn json_value_string() {
    let json = r#"["foo", "\u00a3", "\""]"#;
    let v = JsonValue::parse(json.as_bytes()).unwrap();

    let expected = JsonValue::Array(Arc::new(smallvec![
        JsonValue::Str("foo".to_string()),
        JsonValue::Str("£".to_string()),
        JsonValue::Str("\"".to_string())
    ]));
    assert_eq!(v, expected);
}

#[test]
fn parse_array_3() {
    let json = r#"[1   , null, true]"#;
    let v = JsonValue::parse(json.as_bytes()).unwrap();
    assert_eq!(
        v,
        JsonValue::Array(Arc::new(smallvec![
            JsonValue::Int(1),
            JsonValue::Null,
            JsonValue::Bool(true)
        ]))
    );
}

#[test]
fn parse_array_empty() {
    let json = r#"[   ]"#;
    let v = JsonValue::parse(json.as_bytes()).unwrap();
    assert_eq!(v, JsonValue::Array(Arc::new(smallvec![])));
}

#[test]
fn repeat_trailing_array() {
    let json = "[1]]";
    let e = JsonValue::parse(json.as_bytes()).unwrap_err();
    assert_eq!(e.error_type, JsonErrorType::TrailingCharacters);
    assert_eq!(e.position, FilePosition::new(1, 4));
}

#[test]
fn parse_value_nested() {
    let json = r#"[1, 2, [3, 4], 5, 6]"#;
    let v = JsonValue::parse(json.as_bytes()).unwrap();
    assert_eq!(
        v,
        JsonValue::Array(Arc::new(smallvec![
            JsonValue::Int(1),
            JsonValue::Int(2),
            JsonValue::Array(Arc::new(smallvec![JsonValue::Int(3), JsonValue::Int(4)])),
            JsonValue::Int(5),
            JsonValue::Int(6),
        ]),)
    )
}

#[test]
fn test_array_trailing() {
    let json = r#"[1, 2,]"#;
    let e = JsonValue::parse(json.as_bytes()).unwrap_err();
    assert_eq!(e.error_type, JsonErrorType::TrailingComma);
    assert_eq!(e.position, FilePosition::new(1, 7));
    assert_eq!(e.to_string(), "trailing comma at line 1 column 7");
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
    assert_eq!(array[0], JsonValue::Str("JSON Test Pattern pass1".to_string()));
}

#[test]
fn jiter_object() {
    let mut jiter = Jiter::new(br#"{"foo": "bar", "spam": [   1, 2, "x"]}"#);
    assert_eq!(jiter.next_object().unwrap(), Some("foo"));
    assert_eq!(jiter.next_str().unwrap(), "bar");
    assert_eq!(jiter.next_key().unwrap(), Some("spam"));
    assert_eq!(jiter.next_array().unwrap(), Some(Peak::Num(b'1')));
    assert_eq!(jiter.next_int().unwrap(), NumberInt::Int(1));
    assert_eq!(jiter.array_step().unwrap(), Some(Peak::Num(b'2')));
    assert_eq!(jiter.next_int().unwrap(), NumberInt::Int(2));
    assert_eq!(jiter.array_step().unwrap(), Some(Peak::String));
    assert_eq!(jiter.next_bytes().unwrap(), b"x");
    assert!(jiter.array_step().unwrap().is_none());
    assert_eq!(jiter.next_key().unwrap(), None);
    jiter.finish().unwrap();
}

#[test]
fn jiter_bool() {
    let mut jiter = Jiter::new(b"[true, false, null]");
    assert_eq!(jiter.next_array().unwrap(), Some(Peak::True));
    assert_eq!(jiter.next_bool().unwrap(), true);
    assert_eq!(jiter.array_step().unwrap(), Some(Peak::False));
    assert_eq!(jiter.next_bool().unwrap(), false);
    assert_eq!(jiter.array_step().unwrap(), Some(Peak::Null));
    jiter.next_null().unwrap();
    assert_eq!(jiter.array_step().unwrap(), None);
    jiter.finish().unwrap();
}

#[test]
fn jiter_bytes() {
    let mut jiter = Jiter::new(br#"{"foo": "bar", "new-line": "\\n"}"#);
    assert_eq!(jiter.next_object_bytes().unwrap().unwrap(), b"foo");
    assert_eq!(jiter.next_bytes().unwrap(), b"bar");
    assert_eq!(jiter.next_key_bytes().unwrap().unwrap(), b"new-line");
    assert_eq!(jiter.next_bytes().unwrap(), br#"\\n"#);
    assert_eq!(jiter.next_key_bytes().unwrap(), None);
    jiter.finish().unwrap();
}
#[test]
fn jiter_number() {
    let mut jiter = Jiter::new(br#"  [1, 2.2, 3, 4.1, 5.67]"#);
    assert_eq!(jiter.next_array().unwrap(), Some(Peak::Num(b'1')));
    assert_eq!(jiter.next_int().unwrap(), NumberInt::Int(1));
    assert_eq!(jiter.array_step().unwrap(), Some(Peak::Num(b'2')));
    assert_eq!(jiter.next_float().unwrap(), 2.2);
    assert_eq!(jiter.array_step().unwrap(), Some(Peak::Num(b'3')));
    assert_eq!(jiter.next_number().unwrap(), NumberAny::Int(NumberInt::Int(3)));
    assert_eq!(jiter.array_step().unwrap(), Some(Peak::Num(b'4')));
    assert_eq!(jiter.next_number().unwrap(), NumberAny::Float(4.1));
    assert_eq!(jiter.array_step().unwrap(), Some(Peak::Num(b'5')));
    assert_eq!(jiter.next_number_bytes().unwrap(), b"5.67");
    assert_eq!(jiter.array_step().unwrap(), None);
    jiter.finish().unwrap();
}

#[test]
fn jiter_bytes_u_escape() {
    let mut jiter = Jiter::new(br#"{"foo": "xx \u00a3"}"#);
    assert_eq!(jiter.next_object_bytes().unwrap().unwrap(), b"foo");
    let e = jiter.next_bytes().unwrap_err();
    assert_eq!(
        e.error_type,
        JiterErrorType::JsonError(JsonErrorType::StringEscapeNotSupported)
    );
    assert_eq!(jiter.error_position(&e), FilePosition::new(1, 14));
    assert_eq!(
        e.with_position(&jiter).to_string(),
        "string escape sequences are not supported at line 1 column 14"
    )
}

#[test]
fn jiter_empty_array() {
    let mut jiter = Jiter::new(b"[]");
    assert_eq!(jiter.next_array().unwrap(), None);
    jiter.finish().unwrap();
}

#[test]
fn jiter_trailing_bracket() {
    let mut jiter = Jiter::new(b"[1]]");
    assert_eq!(jiter.next_array().unwrap(), Some(Peak::Num(b'1')));
    assert_eq!(jiter.next_int().unwrap(), NumberInt::Int(1));
    assert!(jiter.array_step().unwrap().is_none());
    let e = jiter.finish().unwrap_err();
    assert_eq!(
        e.error_type,
        JiterErrorType::JsonError(JsonErrorType::TrailingCharacters)
    );
    assert_eq!(jiter.error_position(&e), FilePosition::new(1, 4));
}

#[test]
fn jiter_wrong_type() {
    let mut jiter = Jiter::new(b" 123");
    let e = jiter.next_str().unwrap_err();
    assert_eq!(
        e.error_type,
        JiterErrorType::WrongType {
            expected: JsonType::String,
            actual: JsonType::Int
        }
    );
    assert_eq!(e.index, 1);
    assert_eq!(jiter.error_position(&e), FilePosition::new(1, 2));
    assert_eq!(e.to_string(), "expected string but found int at index 1");
    assert_eq!(
        e.with_position(&jiter).to_string(),
        "expected string but found int at line 1 column 2"
    );
}

#[test]
fn test_crazy_massive_int() {
    let mut s = "5".to_string();
    s.push_str(&"0".repeat(500));
    s.push_str("E-6666");
    let mut jiter = Jiter::new(s.as_bytes());
    assert_eq!(jiter.next_float().unwrap(), 0.0);
    jiter.finish().unwrap();
}

#[test]
fn unique_iter_object() {
    let value = JsonValue::parse(br#" {"x": 1, "x": 2} "#).unwrap();
    if let JsonValue::Object(obj) = value {
        assert_eq!(obj.len(), 1);
        let mut unique = obj.iter_unique();
        let first = unique.next().unwrap();
        assert_eq!(first.0, "x");
        assert_eq!(first.1, &JsonValue::Int(2));
        assert!(unique.next().is_none());
    } else {
        panic!("expected object");
    }
}

#[test]
fn unique_iter_object_repeat() {
    let value = JsonValue::parse(br#" {"x": 1, "x": 1} "#).unwrap();
    if let JsonValue::Object(obj) = value {
        assert_eq!(obj.len(), 1);
        let mut unique = obj.iter_unique();
        let first = unique.next().unwrap();
        assert_eq!(first.0, "x");
        assert_eq!(first.1, &JsonValue::Int(1));
        assert!(unique.next().is_none());
    } else {
        panic!("expected object");
    }
}

#[test]
fn test_recursion_limit() {
    let json = (0..2000).map(|_| "[").collect::<String>();
    let bytes = json.as_bytes();
    let e = JsonValue::parse(bytes).unwrap_err();
    assert_eq!(e.error_type, JsonErrorType::RecursionLimitExceeded);
    assert_eq!(e.index, 201);
    assert_eq!(e.to_string(), "recursion limit exceeded at line 1 column 202");
}

#[test]
fn test_recursion_limit_incr() {
    let json = (0..2000).map(|_| "[1]".to_string()).collect::<Vec<_>>().join(", ");
    let json = format!("[{}]", json);
    let bytes = json.as_bytes();
    let value = JsonValue::parse(bytes).unwrap();
    match value {
        JsonValue::Array(v) => {
            assert_eq!(v.len(), 2000);
        }
        _ => panic!("expected array"),
    }
}

macro_rules! number_bytes {
    ($($name:ident: $json:literal => $expected:expr;)*) => {
        $(
            paste::item! {
                #[test]
                fn [< $name >]() {
                    let mut jiter = Jiter::new($json);
                    let bytes = jiter.next_number_bytes().unwrap();
                    assert_eq!(bytes, $expected);
                }
            }
        )*
    }
}

number_bytes! {
    number_bytes_int: b" 123 " => b"123";
    number_bytes_float: b" 123.456 " => b"123.456";
    number_bytes_zero_float: b" 0.456 " => b"0.456";
    number_bytes_zero: b" 0" => b"0";
    number_bytes_exp: b" 123e4 " => b"123e4";
    number_bytes_exp_neg: b" 123e-4 " => b"123e-4";
    number_bytes_exp_pos: b" 123e+4 " => b"123e+4";
    number_bytes_exp_decimal: b" 123.456e4 " => b"123.456e4";
}

#[test]
fn test_4300_int() {
    let json = (0..4300).map(|_| "9".to_string()).collect::<Vec<_>>().join("");
    let bytes = json.as_bytes();
    let value = JsonValue::parse(bytes).unwrap();
    let expected_big_int = BigInt::from_str(&json).unwrap();
    match value {
        JsonValue::BigInt(v) => {
            assert_eq!(v, expected_big_int);
        }
        _ => panic!("expected array, got {:?}", value),
    }
}

#[test]
fn test_4302_int_err() {
    let json = (0..4302).map(|_| "9".to_string()).collect::<Vec<_>>().join("");
    let bytes = json.as_bytes();
    let e = JsonValue::parse(bytes).unwrap_err();
    assert_eq!(e.error_type, JsonErrorType::NumberOutOfRange);
    assert_eq!(e.index, 4301);
    assert_eq!(e.to_string(), "number out of range at line 1 column 4302");
}
