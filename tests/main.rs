use num_bigint::BigInt;
use std::fs::File;
use std::io::Read;
use std::str::FromStr;
use std::sync::Arc;

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

                        // let r = serde_json::from_str::<serde_json::Value>($json);
                        // let serde_position = match r {
                        //     Ok(v) => panic!("serde unexpectedly valid: {:?} -> {:?}", $json, v),
                        //     Err(e) => (e.line(), e.column()),
                        // };
                        // let jiter_position = (position.line, position.column);
                        // assert_eq!(
                        //     jiter_position,
                        //     serde_position,
                        //     "jiter position {:?} doesn't match serde position {:?}",
                        //     jiter_position,
                        //     serde_position
                        // );
                        return
                    },
                };
                let result = parser.finish();
                match result {
                    Ok(_) => panic!("unexpectedly valid at finish: {:?} -> {:?}", $json, first_value),
                    Err(e) => {
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
    string: ok => r#""foobar""#, "String(1..7) @ 1:0";
    int_pos: ok => "1234", "Int(1234) @ 1:0";
    int_neg: ok => "-1234", "Int(-1234) @ 1:0";
    big_int: ok => "92233720368547758070", "BigInt(92233720368547758070) @ 1:0";
    big_int_neg: ok => "-92233720368547758070", "BigInt(-92233720368547758070) @ 1:0";
    big_int2: ok => "99999999999999999999999999999999999999999999999999", "BigInt(99999999999999999999999999999999999999999999999999) @ 1:0";
    float_pos: ok => "12.34", "Float(12.34) @ 1:0";
    float_neg: ok => "-12.34", "Float(-12.34) @ 1:0";
    float_exp: ok => "2.2e10", "Float(22000000000) @ 1:0";
    float_simple_exp: ok => "20e10", "Float(200000000000) @ 1:0";
    float_exp_pos: ok => "2.2e+10", "Float(22000000000) @ 1:0";
    float_exp_neg: ok => "2.2e-2", "Float(0.022) @ 1:0";
    float_exp_zero: ok => "0.000e123", "Float(0) @ 1:0";
    float_exp_massive1: ok => "2e2147483647", "Float(inf) @ 1:0";
    float_exp_massive2: ok => "2e2147483648", "Float(inf) @ 1:0";
    float_exp_massive3: ok => "2e2147483646", "Float(inf) @ 1:0";
    float_exp_massive4: ok => "2e2147483646", "Float(inf) @ 1:0";
    float_exp_massive5: ok => "18446744073709551615000.0", "Float(18446744073709552000000) @ 1:0";
    float_exp_massive6: ok => "0.0E667", "Float(0) @ 1:0";
    float_exp_tiny0: ok => "2e-2147483647", "Float(0) @ 1:0";
    float_exp_tiny1: ok => "2e-2147483648", "Float(0) @ 1:0";
    float_exp_tiny2: ok => "2e-2147483646", "Float(0) @ 1:0";
    float_exp_tiny3: ok => "8e-7766666666", "Float(0) @ 1:0";
    float_exp_tiny4: ok => "200.08e-76200000102", "Float(0) @ 1:0";
    float_exp_tiny5: ok => "0e459", "Float(0) @ 1:0";
    null: ok => "null", "null @ 1:0";
    v_true: ok => "true", "true @ 1:0";
    v_false: ok => "false", "false @ 1:0";
    offset_true: ok => "  true", "true @ 1:2";
    empty: err => "", "EofWhileParsingValue @ 1:0";
    string_unclosed: err => r#""foobar"#, "EofWhileParsingString @ 1:7";
    bad_int: err => "-", "EofWhileParsingValue @ 1:1";
    bad_true1: err => "truX", "ExpectedSomeIdent @ 1:3";
    bad_true2: err => "tru", "EofWhileParsingValue @ 1:3";
    bad_true3: err => "trX", "ExpectedSomeIdent @ 1:2";
    bad_false1: err => "falsX", "ExpectedSomeIdent @ 1:4";
    bad_false2: err => "fals", "EofWhileParsingValue @ 1:4";
    bad_null1: err => "nulX", "ExpectedSomeIdent @ 1:3";
    bad_null2: err => "nul", "EofWhileParsingValue @ 1:3";
    object_trailing_comma: err => r#"{"foo": "bar",}"#, "TrailingComma @ 1:15";
    array_trailing_comma: err => r#"[1, 2,]"#, "TrailingComma @ 1:7";
    array_wrong_char_after_comma: err => r#"[1, 2,;"#, "ExpectedSomeValue @ 1:7";
    array_end_after_comma: err => "[9,", "EofWhileParsingValue @ 1:3";
    object_wrong_char: err => r#"{"foo":42;"#, "ExpectedObjectCommaOrEnd @ 1:10";
    object_wrong_char_after_comma: err => r#"{"foo":42,;"#, "KeyMustBeAString @ 1:11";
    object_end_after_comma: err => r#"{"x": 9,"#, "EofWhileParsingValue @ 1:8";
    object_end_after_colon: err => r#"{"":"#, "EofWhileParsingValue @ 1:4";
    array_bool: ok => "[true, false]", "[ @ 1:0, true @ 1:1, false @ 1:7, ]";
    object_string: ok => r#"{"foo": "ba"}"#, "{ @ 1:0, Key(2..5), String(9..11) @ 1:8, }";
    object_null: ok => r#"{"foo": null}"#, "{ @ 1:0, Key(2..5), null @ 1:8, }";
    object_bool_compact: ok => r#"{"foo":true}"#, "{ @ 1:0, Key(2..5), true @ 1:7, }";
    deep_array: ok => r#"[["Not too deep"]]"#, "[ @ 1:0, [ @ 1:1, String(3..15) @ 1:2, ], ]";
    object_key_int: err => r#"{4: 4}"#, "KeyMustBeAString @ 1:2";
    array_no_close: err => r#"["#, "EofWhileParsingList @ 1:1";
    array_double_close: err => "[1]]", "TrailingCharacters @ 1:3";
    double_zero: err => "001", "InvalidNumber @ 1:0";
    invalid_float_e_end: err => "0E", "EofWhileParsingValue @ 1:2";
    invalid_float_dot_end: err => "0.", "EofWhileParsingValue @ 1:2";
    invalid_float_bracket: err => "2E[", "InvalidNumber @ 1:2";
}

#[test]
fn invalid_string_controls() {
    let json = "\"123\x08\x0c\n\r\t\"";
    let mut tape: Vec<u8> = Vec::new();
    let b = json.as_bytes();
    let mut parser = Parser::new(b);
    let peak = parser.peak().unwrap();
    assert!(matches!(peak, Peak::String));
    let result = parser.consume_string::<StringDecoder>(&mut tape);
    match result {
        Ok(t) => panic!("unexpectedly valid: {:?} -> {:?}", json, t),
        Err(e) => {
            assert_eq!(e.index, 0);
            assert_eq!(e.error_type, JsonErrorType::ControlCharacterWhileParsingString(3))
        }
    }
}

#[test]
fn json_parse_str() {
    let json = r#" "foobar" "#;
    let mut tape: Vec<u8> = Vec::new();
    let data = json.as_bytes();
    let mut parser = Parser::new(data);
    let peak = parser.peak().unwrap();
    assert!(matches!(peak, Peak::String));
    assert_eq!(parser.current_position(), FilePosition::new(1, 1));

    let result_string = parser.consume_string::<StringDecoder>(&mut tape).unwrap();
    assert_eq!(result_string, "foobar");
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
                    let result_string = parser.consume_string::<StringDecoder>(&mut tape).unwrap();
                    assert_eq!(result_string, $expected);
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

#[test]
fn test_key_str() {
    let json = r#"{"foo": "bar"}"#;
    let mut tape: Vec<u8> = Vec::new();
    let mut parser = Parser::new(json.as_bytes());
    let p = parser.peak().unwrap();
    assert!(matches!(p, Peak::Object));
    let k = parser.object_first::<StringDecoder>(&mut tape).unwrap();
    assert_eq!(k, Some("foo"));
    let p = parser.peak().unwrap();
    assert!(matches!(p, Peak::String));
    let v = parser.consume_string::<StringDecoder>(&mut tape).unwrap();
    assert_eq!(v, "bar");
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
    first_line_zero: b"123456", 0, FilePosition::new(1, 0);
    first_line_first: b"123456", 1, FilePosition::new(1, 1);
    first_line_3rd: b"123456", 3, FilePosition::new(1, 3);
    first_line_last: b"123456", 6, FilePosition::new(1, 6);
    first_line_after: b"123456", 7, FilePosition::new(1, 6);
    first_line_last2: b"123456\n789", 6, FilePosition::new(1, 6);
    second_line: b"123456\n789", 7, FilePosition::new(2, 0);
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
    let r = JsonValue::parse(&bytes);
    match r {
        Ok(v) => panic!("unexpected valid {v:?}"),
        Err(e) => {
            assert_eq!(e.index, 0);
            assert_eq!(e.error_type, JsonErrorType::ControlCharacterWhileParsingString(0))
        }
    };
}

#[test]
fn bad_high_order_string() {
    let bytes: Vec<u8> = vec![34, 32, 32, 210, 34];
    let r = JsonValue::parse(&bytes);
    match r {
        Ok(v) => panic!("unexpected valid {v:?}"),
        Err(e) => {
            assert_eq!(e.error_type, JsonErrorType::InvalidUnicodeCodePoint(2));
            assert_eq!(e.index, 0);
            assert_eq!(e.position, FilePosition::new(1, 0));
        }
    };
}

#[test]
fn udb_string() {
    let bytes: Vec<u8> = vec![34, 92, 117, 100, 66, 100, 100, 92, 117, 100, 70, 100, 100, 34];
    let v = JsonValue::parse(&bytes).unwrap();
    match v {
        JsonValue::String(s) => assert_eq!(s.as_bytes(), [244, 135, 159, 157]),
        _ => panic!("unexpected valid {v:?}"),
    }
}

#[test]
fn parse_object() {
    let json = r#"{"foo": "bar", "spam": [1, null, true]}"#;
    let v = JsonValue::parse(json.as_bytes()).unwrap();

    let mut expected = LazyIndexMap::new();
    expected.insert("foo".to_string(), JsonValue::String("bar".to_string()));
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
    let result = JsonValue::parse(json.as_bytes());
    match result {
        Ok(t) => panic!("unexpectedly valid: {:?} -> {:?}", json, t),
        Err(e) => {
            assert_eq!(e.error_type, JsonErrorType::TrailingCharacters);
            // assert_eq!(e.position, FilePosition::new(1, 4));
        }
    }
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
    let result = JsonValue::parse(json.as_bytes());
    match result {
        Ok(t) => panic!("unexpectedly valid: {:?} -> {:?}", json, t),
        Err(e) => {
            // assert_eq!(e.to_string(), "");
            assert_eq!(e.error_type, JsonErrorType::TrailingComma);
            assert_eq!(e.position, FilePosition::new(1, 7));
        }
    }
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
    match jiter.next_bytes() {
        Ok(r) => panic!("unexpectedly valid: {:?}", r),
        Err(e) => {
            assert_eq!(
                e.error_type,
                JiterErrorType::JsonError(JsonErrorType::StringEscapeNotSupported(4))
            );
            assert_eq!(jiter.error_position(&e), FilePosition::new(1, 8));
        }
    }
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
    let result = jiter.finish();
    match result {
        Ok(t) => panic!("unexpectedly valid: {:?}", t),
        Err(e) => {
            assert_eq!(
                e.error_type,
                JiterErrorType::JsonError(JsonErrorType::TrailingCharacters)
            );
            assert_eq!(jiter.error_position(&e), FilePosition::new(1, 3));
        }
    }
}

#[test]
fn jiter_wrong_type() {
    let mut jiter = Jiter::new(b" 123");
    let result = jiter.next_str();
    match result {
        Ok(t) => panic!("unexpectedly valid: {:?}", t),
        Err(e) => {
            assert_eq!(
                e.error_type,
                JiterErrorType::WrongType {
                    expected: JsonType::String,
                    actual: JsonType::Int,
                }
            );
            assert_eq!(e.index, 1);
            assert_eq!(jiter.error_position(&e), FilePosition::new(1, 1));
        }
    }
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
    match JsonValue::parse(bytes) {
        Ok(v) => panic!("unexpectedly valid: {:?}", v),
        Err(e) => {
            assert_eq!(e.error_type, JsonErrorType::RecursionLimitExceeded);
            assert_eq!(e.index, 201);
        }
    }
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
    match JsonValue::parse(bytes) {
        Ok(v) => panic!("unexpectedly valid: {:?}", v),
        Err(e) => {
            assert_eq!(e.error_type, JsonErrorType::NumberOutOfRange);
            assert_eq!(e.index, 4301);
        }
    }
}
