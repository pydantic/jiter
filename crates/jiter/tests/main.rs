#![allow(clippy::float_cmp)]

use std::borrow::Cow;
use std::fs::File;
use std::io::Read;
use std::str::FromStr;
use std::sync::Arc;

#[cfg(feature = "num-bigint")]
use num_bigint::BigInt;

use jiter::{
    Jiter, JiterErrorType, JiterResult, JsonErrorType, JsonObject, JsonType, JsonValue, LinePosition, NumberAny,
    NumberInt, PartialMode, Peek,
};

fn json_vec(jiter: &mut Jiter, peek: Option<Peek>) -> JiterResult<Vec<String>> {
    let mut v = Vec::new();
    let peek = match peek {
        Some(peek) => peek,
        None => jiter.peek()?,
    };

    let position = jiter.current_position().short();
    match peek {
        Peek::True => {
            jiter.known_bool(peek)?;
            v.push(format!("true @ {position}"));
        }
        Peek::False => {
            jiter.known_bool(peek)?;
            v.push(format!("false @ {position}"));
        }
        Peek::Null => {
            jiter.known_null()?;
            v.push(format!("null @ {position}"));
        }
        Peek::String => {
            let str = jiter.known_str()?;
            v.push(format!("String({str}) @ {position}"));
        }
        Peek::Array => {
            v.push(format!("[ @ {position}"));
            if let Some(peek) = jiter.known_array()? {
                let el_vec = json_vec(jiter, Some(peek))?;
                v.extend(el_vec);
                while let Some(peek) = jiter.array_step()? {
                    let el_vec = json_vec(jiter, Some(peek))?;
                    v.extend(el_vec);
                }
            }
            v.push("]".to_string());
        }
        Peek::Object => {
            v.push(format!("{{ @ {position}"));
            if let Some(key) = jiter.known_object()? {
                v.push(format!("Key({key})"));
                let value_vec = json_vec(jiter, None)?;
                v.extend(value_vec);
                while let Some(key) = jiter.next_key()? {
                    v.push(format!("Key({key}"));
                    let value_vec = json_vec(jiter, None)?;
                    v.extend(value_vec);
                }
            }
            v.push("}".to_string());
        }
        _ => {
            let s = display_number(peek, jiter)?;
            v.push(s);
        }
    }
    Ok(v)
}

fn display_number(peek: Peek, jiter: &mut Jiter) -> JiterResult<String> {
    let position = jiter.current_position().short();
    let number = jiter.known_number(peek)?;
    let s = match number {
        NumberAny::Int(NumberInt::Int(int)) => {
            format!("Int({int}) @ {position}")
        }
        #[cfg(feature = "num-bigint")]
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
                let mut jiter = Jiter::new($json.as_bytes()).with_allow_inf_nan();
                let elements = json_vec(&mut jiter, None).unwrap().join(", ");
                assert_eq!(elements, $expected);
                jiter.finish().unwrap();
            }
        }
    };

    ($name:ident, err, $json:literal, $expected_error:literal) => {
        paste::item! {
            #[allow(non_snake_case)]
            #[test]
            fn [< single_element_xerror__ $name >]() {
                let mut jiter = Jiter::new($json.as_bytes()).with_allow_inf_nan();
                let result = json_vec(&mut jiter, None);
                let first_value = match result {
                    Ok(v) => v,
                    Err(e) => {
                        let position = jiter.error_position(e.index);
                        // no wrong type errors, so unwrap the json error
                        let JiterErrorType::JsonError(error_type) = e.error_type else {
                            panic!("unexpected error type: {:?}", e.error_type);
                        };
                        let actual_error = format!("{:?} @ {}", error_type, position.short());
                        assert_eq!(actual_error, $expected_error);

                        let full_error = format!("{} at {}", error_type, position);
                        let serde_err = serde_json::from_str::<serde_json::Value>($json).unwrap_err();
                        assert_eq!(full_error, serde_err.to_string());
                        return
                    },
                };
                let result = jiter.finish();
                match result {
                    Ok(_) => panic!("unexpectedly valid at finish: {:?} -> {:?}", $json, first_value),
                    Err(e) => {
                        // to check to_string works, and for coverage
                        e.to_string();
                        let position = jiter.error_position(e.index);
                        // no wrong type errors, so unwrap the json error
                        let JiterErrorType::JsonError(error_type) = e.error_type else {
                            panic!("unexpected error type: {:?}", e.error_type);
                        };
                        let actual_error = format!("{:?} @ {}", error_type, position.short());
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
    string: ok => r#""foobar""#, "String(foobar) @ 1:1";
    int_pos: ok => "1234", "Int(1234) @ 1:1";
    int_zero: ok => "0", "Int(0) @ 1:1";
    int_zero_space: ok => "0 ", "Int(0) @ 1:1";
    int_neg: ok => "-1234", "Int(-1234) @ 1:1";
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
    nan: ok => "NaN", "Float(NaN) @ 1:1";
    infinity: ok => "Infinity", "Float(inf) @ 1:1";
    neg_infinity: ok => "-Infinity", "Float(-inf) @ 1:1";
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
    array_trailing_comma: err => r"[1, 2,]", "TrailingComma @ 1:7";
    array_wrong_char_after_comma: err => r"[1, 2,;", "ExpectedSomeValue @ 1:7";
    array_end_after_comma: err => "[9,", "EofWhileParsingValue @ 1:3";
    object_wrong_char: err => r#"{"foo":42;"#, "ExpectedObjectCommaOrEnd @ 1:10";
    object_wrong_char_after_comma: err => r#"{"foo":42,;"#, "KeyMustBeAString @ 1:11";
    object_end_after_comma: err => r#"{"x": 9,"#, "EofWhileParsingValue @ 1:8";
    object_end_after_colon: err => r#"{"":"#, "EofWhileParsingValue @ 1:4";
    eof_while_parsing_object: err => r#"{"foo": 1"#, "EofWhileParsingObject @ 1:9";
    expected_colon: err => r#"{"foo"1"#, "ExpectedColon @ 1:7";
    array_bool: ok => "[true, false]", "[ @ 1:1, true @ 1:2, false @ 1:8, ]";
    object_string: ok => r#"{"foo": "ba"}"#, "{ @ 1:1, Key(foo), String(ba) @ 1:9, }";
    object_null: ok => r#"{"foo": null}"#, "{ @ 1:1, Key(foo), null @ 1:9, }";
    object_bool_compact: ok => r#"{"foo":true}"#, "{ @ 1:1, Key(foo), true @ 1:8, }";
    deep_array: ok => r#"[["Not too deep"]]"#, "[ @ 1:1, [ @ 1:2, String(Not too deep) @ 1:3, ], ]";
    object_key_int: err => r"{4: 4}", "KeyMustBeAString @ 1:2";
    array_no_close: err => r"[", "EofWhileParsingList @ 1:1";
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
    unexpect_value_xx: err => "xx", "ExpectedSomeValue @ 1:1";
}

#[cfg(feature = "num-bigint")]
single_tests! {
    big_int: ok => "92233720368547758070", "BigInt(92233720368547758070) @ 1:1";
    big_int_neg: ok => "-92233720368547758070", "BigInt(-92233720368547758070) @ 1:1";
    big_int2: ok => "99999999999999999999999999999999999999999999999999", "BigInt(99999999999999999999999999999999999999999999999999) @ 1:1";
    float_exp_massive5: ok => "18446744073709551615000.0", "Float(18446744073709552000000) @ 1:1";
}

#[test]
fn invalid_string_controls() {
    let json = "\"123\x08\x0c\n\r\t\"";
    let b = json.as_bytes();
    let mut jiter = Jiter::new(b);
    let e = jiter.next_str().unwrap_err();
    assert_eq!(
        e.error_type,
        JiterErrorType::JsonError(JsonErrorType::ControlCharacterWhileParsingString)
    );
    assert_eq!(e.index, 4);
    assert_eq!(jiter.error_position(e.index), LinePosition::new(1, 5));
    assert_eq!(
        e.to_string(),
        "control character (\\u0000-\\u001F) found while parsing a string at index 4"
    );
}

#[test]
fn json_parse_str() {
    let json = r#" "foobar" "#;
    let data = json.as_bytes();
    let mut jiter = Jiter::new(data);
    let peek = jiter.peek().unwrap();
    assert_eq!(peek, Peek::String);
    assert_eq!(jiter.current_position(), LinePosition::new(1, 2));

    let result_string = jiter.known_str().unwrap();
    assert_eq!(result_string, "foobar");
    jiter.finish().unwrap();
}

macro_rules! string_tests {
    ($($name:ident: $json:literal => $expected:expr;)*) => {
        $(
            paste::item! {
                #[test]
                fn [< string_parsing_ $name >]() {
                    let data = $json.as_bytes();
                    let mut jiter = Jiter::new(data);
                    let str = jiter.next_str().unwrap();
                    assert_eq!(str, $expected);
                    jiter.finish().unwrap();
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
                    let mut jiter = Jiter::new(data);
                    match jiter.next_str() {
                        Ok(t) => panic!("unexpectedly valid: {:?} -> {:?}", $json, t),
                        Err(e) => {
                            // to check to_string works, and for coverage
                            e.to_string();
                            let JiterErrorType::JsonError(error_type) = e.error_type else {
                                panic!("unexpected error type: {:?}", e.error_type);
                            };
                            let position = jiter.error_position(e.index);
                            let actual_error = format!("{:?} @ {} - {}", error_type, e.index, position.short());
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
    // needs the long tail so SIMD is used to parse the string
    line_leading_surrogate_tail: r#""\uddBd"                     "# => "LoneLeadingSurrogateInHexEscape @ 6 - 1:7";
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
    let mut jiter = Jiter::new(&json);
    let e = jiter.next_str().unwrap_err();
    assert_eq!(
        e.error_type,
        JiterErrorType::JsonError(JsonErrorType::InvalidUnicodeCodePoint)
    );
    assert_eq!(e.index, 3);
    assert_eq!(jiter.error_position(e.index), LinePosition::new(1, 4));
}

#[test]
fn invalid_control() {
    let json = vec![34, 206, 34];
    // dbg!(json.iter().map(|b| *b as char).collect::<Vec<_>>());
    let mut jiter = Jiter::new(&json);
    let e = jiter.next_str().unwrap_err();
    assert_eq!(
        e.error_type,
        JiterErrorType::JsonError(JsonErrorType::InvalidUnicodeCodePoint)
    );
    assert_eq!(e.index, 2);
    assert_eq!(jiter.error_position(e.index), LinePosition::new(1, 3));
}

#[test]
fn utf8_range() {
    for c in 0u8..255u8 {
        let json = vec![34, c, 34];
        // dbg!(c, json.iter().map(|b| *b as char).collect::<Vec<_>>());

        let jiter_result = JsonValue::parse(&json, false);
        match serde_json::from_slice::<String>(&json) {
            Ok(serde_s) => {
                let jiter_value = jiter_result.unwrap();
                assert_eq!(jiter_value, JsonValue::Str(serde_s.into()));
            }
            Err(serde_err) => {
                let jiter_err = jiter_result.unwrap_err();
                let position = jiter_err.get_position(&json);
                let full_error = format!("{} at {position}", jiter_err.error_type);
                assert_eq!(full_error, serde_err.to_string());
            }
        }
    }
}

#[test]
fn utf8_range_long() {
    for c in 0u8..255u8 {
        let mut json = vec![b'"', b':', c];
        json.extend(std::iter::repeat(b' ').take(20));
        json.push(b'"');
        // dbg!(c, json.iter().map(|b| *b as char).collect::<Vec<_>>());

        let jiter_result = JsonValue::parse(&json, false);
        match serde_json::from_slice::<String>(&json) {
            Ok(serde_s) => {
                let jiter_value = jiter_result.unwrap();
                assert_eq!(jiter_value, JsonValue::Str(serde_s.into()));
            }
            Err(serde_err) => {
                let jiter_err = jiter_result.unwrap_err();
                // just compare the start of the error - https://github.com/serde-rs/json/issues/1110
                assert!(serde_err.to_string().starts_with(&jiter_err.error_type.to_string()));
            }
        }
    }
}

#[test]
fn nan_disallowed() {
    let json = r"[NaN]";
    let mut jiter = Jiter::new(json.as_bytes());
    assert_eq!(jiter.next_array().unwrap().unwrap(), Peek::NaN);
    let e = jiter.next_number().unwrap_err();
    assert_eq!(
        e.error_type,
        JiterErrorType::JsonError(JsonErrorType::ExpectedSomeValue)
    );
    assert_eq!(e.index, 1);
    assert_eq!(jiter.error_position(e.index), LinePosition::new(1, 2));
}

#[test]
fn inf_disallowed() {
    let json = r"[Infinity]";
    let mut jiter = Jiter::new(json.as_bytes());
    assert_eq!(jiter.next_array().unwrap().unwrap(), Peek::Infinity);
    let e = jiter.next_number().unwrap_err();
    assert_eq!(
        e.error_type,
        JiterErrorType::JsonError(JsonErrorType::ExpectedSomeValue)
    );
    assert_eq!(e.index, 1);
    assert_eq!(jiter.error_position(e.index), LinePosition::new(1, 2));
}

#[test]
fn inf_neg_disallowed() {
    let json = r"[-Infinity]";
    let mut jiter = Jiter::new(json.as_bytes());
    assert_eq!(jiter.next_array().unwrap().unwrap(), Peek::Minus);
    let e = jiter.next_number().unwrap_err();
    assert_eq!(e.error_type, JiterErrorType::JsonError(JsonErrorType::InvalidNumber));
    assert_eq!(e.index, 2);
    assert_eq!(jiter.error_position(e.index), LinePosition::new(1, 3));
}

#[test]
fn num_after() {
    let json = r"2:"; // `:` is 58, directly after 9
    let mut jiter = Jiter::new(json.as_bytes());
    let num = jiter.next_number().unwrap();
    assert_eq!(num, NumberAny::Int(NumberInt::Int(2)));
    let e = jiter.finish().unwrap_err();
    assert_eq!(
        e.error_type,
        JiterErrorType::JsonError(JsonErrorType::TrailingCharacters)
    );
    assert_eq!(e.index, 1);
    assert_eq!(jiter.error_position(e.index), LinePosition::new(1, 2));
}

#[test]
fn num_before() {
    let json = r"2/"; // `/` is 47, directly before 0
    let mut jiter = Jiter::new(json.as_bytes());
    let num = jiter.next_number().unwrap();
    assert_eq!(num, NumberAny::Int(NumberInt::Int(2)));
    let e = jiter.finish().unwrap_err();
    assert_eq!(
        e.error_type,
        JiterErrorType::JsonError(JsonErrorType::TrailingCharacters)
    );
    assert_eq!(e.index, 1);
    assert_eq!(jiter.error_position(e.index), LinePosition::new(1, 2));
}

#[test]
fn nan_disallowed_wrong_type() {
    let json = r"[NaN]";
    let mut jiter = Jiter::new(json.as_bytes());
    assert_eq!(jiter.next_array().unwrap().unwrap(), Peek::NaN);
    let e = jiter.next_str().unwrap_err();
    assert_eq!(
        e.error_type,
        JiterErrorType::JsonError(JsonErrorType::ExpectedSomeValue)
    );
    assert_eq!(e.index, 1);
    assert_eq!(jiter.error_position(e.index), LinePosition::new(1, 2));
}

#[test]
fn value_allow_nan_inf() {
    let json = r"[1, NaN, Infinity, -Infinity]";
    let value = JsonValue::parse(json.as_bytes(), true).unwrap();
    let expected = JsonValue::Array(Arc::new(vec![
        JsonValue::Int(1),
        JsonValue::Float(f64::NAN),
        JsonValue::Float(f64::INFINITY),
        JsonValue::Float(f64::NEG_INFINITY),
    ]));
    // compare debug since `f64::NAN != f64::NAN`
    assert_eq!(format!("{value:?}"), format!("{:?}", expected));
}

#[test]
fn value_disallow_nan() {
    let json = r"[1, NaN]";
    let err = JsonValue::parse(json.as_bytes(), false).unwrap_err();
    assert_eq!(err.error_type, JsonErrorType::ExpectedSomeValue);
    assert_eq!(err.description(json.as_bytes()), "expected value at line 1 column 5");
}

#[test]
fn key_str() {
    let json = r#"{"foo": "bar"}"#;
    let mut jiter = Jiter::new(json.as_bytes());
    assert_eq!(jiter.next_object().unwrap().unwrap(), "foo");
    assert_eq!(jiter.next_str().unwrap(), "bar");
    assert!(jiter.next_key().unwrap().is_none());
    jiter.finish().unwrap();
}

#[test]
fn key_bytes() {
    let json = r#"{"foo": "bar"}"#.as_bytes();
    let mut jiter = Jiter::new(json);
    assert_eq!(jiter.next_object_bytes().unwrap().unwrap(), b"foo");
    assert_eq!(jiter.next_bytes().unwrap(), *b"bar");
    assert!(jiter.next_key().unwrap().is_none());
    jiter.finish().unwrap();
}

macro_rules! test_position {
    ($($name:ident: $data:literal, $find:literal, $expected:expr;)*) => {
        $(
            paste::item! {
                #[test]
                fn [< test_position_ $name >]() {
                    assert_eq!(LinePosition::find($data, $find), $expected);
                }
            }
        )*
    }
}

test_position! {
    empty_zero: b"", 0, LinePosition::new(1, 0);
    empty_one: b"", 1, LinePosition::new(1, 0);
    first_line_zero: b"123456", 0, LinePosition::new(1, 1);
    first_line_first: b"123456", 1, LinePosition::new(1, 2);
    first_line_3rd: b"123456", 3, LinePosition::new(1, 4);
    first_line_5th: b"123456", 5, LinePosition::new(1, 6);
    first_line_last: b"123456", 6, LinePosition::new(1, 6);
    first_line_after: b"123456", 7, LinePosition::new(1, 6);
    second_line0: b"123456\n789", 6, LinePosition::new(2, 0);
    second_line1: b"123456\n789", 7, LinePosition::new(2, 1);
    second_line2: b"123456\n789", 8, LinePosition::new(2, 2);
}

#[test]
fn parse_tiny_float() {
    let v = JsonValue::parse(b"8e-7766666666", false).unwrap();
    assert_eq!(v, JsonValue::Float(0.0));
}

#[test]
fn parse_zero_float() {
    let v = JsonValue::parse(b"0.1234", false).unwrap();
    match v {
        JsonValue::Float(v) => assert!((0.1234 - v).abs() < 1e-6),
        other => panic!("unexpected value: {other:?}"),
    }
}

#[test]
fn parse_zero_exp_float() {
    let v = JsonValue::parse(b"0.12e3", false).unwrap();
    match v {
        JsonValue::Float(v) => assert!((120.0 - v).abs() < 1e-3),
        other => panic!("unexpected value: {other:?}"),
    }
}

#[test]
fn bad_lower_value_in_string() {
    let bytes: Vec<u8> = vec![34, 27, 32, 34];
    let e = JsonValue::parse(&bytes, false).unwrap_err();
    assert_eq!(e.error_type, JsonErrorType::ControlCharacterWhileParsingString);
    assert_eq!(e.index, 1);
    assert_eq!(e.get_position(&bytes), LinePosition::new(1, 2));
}

#[test]
fn bad_high_order_string() {
    let bytes: Vec<u8> = vec![34, 32, 32, 210, 34];
    let e = JsonValue::parse(&bytes, false).unwrap_err();
    assert_eq!(e.error_type, JsonErrorType::InvalidUnicodeCodePoint);
    assert_eq!(e.index, 4);
    assert_eq!(e.description(&bytes), "invalid unicode code point at line 1 column 5");
}

#[test]
fn bad_high_order_string_tail() {
    // needs the long tail so SIMD is used to parse the string
    let mut bytes: Vec<u8> = vec![34, 32, 32, 210, 34];
    bytes.extend(vec![b' '; 100]);
    // dbg!(json.iter().map(|b| *b as char).collect::<Vec<_>>());
    let e = JsonValue::parse(&bytes, false).unwrap_err();
    assert_eq!(e.error_type, JsonErrorType::InvalidUnicodeCodePoint);
    assert_eq!(e.index, 4);
    assert_eq!(e.description(&bytes), "invalid unicode code point at line 1 column 5");
}

#[test]
fn invalid_escape_position() {
    // from fuzzing on #130
    let bytes = br#""con(\u0trol character (\u000.00"#;
    let e = JsonValue::parse(bytes, false).unwrap_err();
    assert_eq!(e.error_type, JsonErrorType::InvalidEscape);
    assert_eq!(e.index, 8);
    assert_eq!(e.description(bytes), "invalid escape at line 1 column 9");
}

#[test]
fn simd_string_sizes() {
    for i in 0..100 {
        let mut json = vec![b'"'];
        json.extend(std::iter::repeat(b'a').take(i));
        json.push(b'"');
        json.extend(std::iter::repeat(b' ').take(40));

        let value = JsonValue::parse(&json, false).unwrap();
        let JsonValue::Str(s) = value else {
            panic!("unexpected value {value:?}");
        };
        assert_eq!(s.len(), i);
        assert!(s.as_bytes().iter().all(|&b| b == b'a'));
    }
}

#[test]
fn udb_string() {
    let bytes: Vec<u8> = vec![34, 92, 117, 100, 66, 100, 100, 92, 117, 100, 70, 100, 100, 34];
    let v = JsonValue::parse(&bytes, false).unwrap();
    match v {
        JsonValue::Str(s) => assert_eq!(s.as_bytes(), [244, 135, 159, 157]),
        _ => panic!("unexpected value {v:?}"),
    }
}

#[test]
fn json_value_object() {
    let json = r#"{"foo": "bar", "spam": [1, null, true]}"#;
    let v = JsonValue::parse(json.as_bytes(), false).unwrap();

    let expected = JsonValue::Object(Arc::new(vec![
        ("foo".into(), JsonValue::Str("bar".into())),
        (
            "spam".into(),
            JsonValue::Array(Arc::new(vec![
                JsonValue::Int(1),
                JsonValue::Null,
                JsonValue::Bool(true),
            ])),
        ),
    ]));
    assert_eq!(v, expected);
}

#[test]
fn json_value_string() {
    let json = r#"["foo", "\u00a3", "\""]"#;
    let v = JsonValue::parse(json.as_bytes(), false).unwrap();

    let expected = JsonValue::Array(Arc::new(vec![
        JsonValue::Str("foo".into()),
        JsonValue::Str("£".into()),
        JsonValue::Str("\"".into()),
    ]));
    assert_eq!(v, expected);
}

#[test]
fn parse_array_3() {
    let json = r"[1   , null, true]";
    let v = JsonValue::parse(json.as_bytes(), false).unwrap();
    assert_eq!(
        v,
        JsonValue::Array(Arc::new(vec![
            JsonValue::Int(1),
            JsonValue::Null,
            JsonValue::Bool(true)
        ]))
    );
}

#[test]
fn parse_array_empty() {
    let json = r"[   ]";
    let v = JsonValue::parse(json.as_bytes(), false).unwrap();
    assert_eq!(v, JsonValue::Array(Arc::new(vec![])));
}

#[test]
fn repeat_trailing_array() {
    let json = b"[1]]";
    let e = JsonValue::parse(json, false).unwrap_err();
    assert_eq!(e.error_type, JsonErrorType::TrailingCharacters);
    assert_eq!(e.get_position(json), LinePosition::new(1, 4));
}

#[test]
fn parse_value_nested() {
    let json = r"[1, 2, [3, 4], 5, 6]";
    let v = JsonValue::parse(json.as_bytes(), false).unwrap();
    assert_eq!(
        v,
        JsonValue::Array(Arc::new(vec![
            JsonValue::Int(1),
            JsonValue::Int(2),
            JsonValue::Array(Arc::new(vec![JsonValue::Int(3), JsonValue::Int(4)])),
            JsonValue::Int(5),
            JsonValue::Int(6),
        ]),)
    );
}

#[test]
fn test_array_trailing() {
    let json = br"[1, 2,]";
    let e = JsonValue::parse(json, false).unwrap_err();
    assert_eq!(e.error_type, JsonErrorType::TrailingComma);
    assert_eq!(e.get_position(json), LinePosition::new(1, 7));
    assert_eq!(e.description(json), "trailing comma at line 1 column 7");
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
    let v = JsonValue::parse(json_data, false).unwrap();
    let array = match v {
        JsonValue::Array(array) => array,
        v => panic!("expected array, not {v:?}"),
    };
    assert_eq!(array.len(), 20);
    assert_eq!(array[0], JsonValue::Str("JSON Test Pattern pass1".into()));
}

#[test]
fn pass1_skip() {
    let json = read_file("./benches/pass1.json");
    let json_data = json.as_bytes();
    let mut jiter = Jiter::new(json_data);
    jiter.next_skip().unwrap();
    jiter.finish().unwrap();
}

#[test]
fn escaped_string() {
    let json_data = br#""&#34; \u0022 %22 0x22 034 &#x22;""#;
    // let json_data = br#"  "\n"  "#;
    let v = JsonValue::parse(json_data, false).unwrap();
    let s = match v {
        JsonValue::Str(s) => s,
        v => panic!("expected array, not {v:?}"),
    };
    drop(s);
    // assert_eq!(s, r#"&#34; " %22 0x22 034 &#x22;"#);
}

#[test]
fn jiter_object() {
    let mut jiter = Jiter::new(br#"{"foo": "bar", "spam": [   1, -2, "x"]}"#);
    assert_eq!(jiter.next_object().unwrap(), Some("foo"));
    assert_eq!(jiter.next_str().unwrap(), "bar");
    assert_eq!(jiter.next_key().unwrap(), Some("spam"));
    assert_eq!(jiter.next_array().unwrap().unwrap().into_inner(), b'1');
    assert_eq!(jiter.next_int().unwrap(), NumberInt::Int(1));
    assert_eq!(jiter.array_step().unwrap(), Some(Peek::Minus));
    assert_eq!(jiter.next_int().unwrap(), NumberInt::Int(-2));
    assert_eq!(jiter.array_step().unwrap(), Some(Peek::String));
    assert_eq!(jiter.next_bytes().unwrap(), b"x");
    assert!(jiter.array_step().unwrap().is_none());
    assert_eq!(jiter.next_key().unwrap(), None);
    jiter.finish().unwrap();
}

#[test]
fn jiter_inf() {
    let mut jiter = Jiter::new(b"[Infinity, -Infinity, NaN]").with_allow_inf_nan();
    assert_eq!(jiter.next_array().unwrap(), Some(Peek::Infinity));
    assert_eq!(jiter.next_float().unwrap(), f64::INFINITY);
    assert_eq!(jiter.array_step().unwrap(), Some(Peek::Minus));
    assert_eq!(jiter.next_float().unwrap(), f64::NEG_INFINITY);
    assert_eq!(jiter.array_step().unwrap(), Some(Peek::NaN));
    assert_eq!(jiter.next_float().unwrap().to_string(), "NaN");
    assert_eq!(jiter.array_step().unwrap(), None);
    jiter.finish().unwrap();
}

#[test]
fn jiter_bool() {
    let mut jiter = Jiter::new(b"[true, false, null]");
    assert_eq!(jiter.next_array().unwrap(), Some(Peek::True));
    assert!(jiter.next_bool().unwrap());
    assert_eq!(jiter.array_step().unwrap(), Some(Peek::False));
    assert!(!jiter.next_bool().unwrap());
    assert_eq!(jiter.array_step().unwrap(), Some(Peek::Null));
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
    assert_eq!(jiter.next_bytes().unwrap(), br"\\n");
    assert_eq!(jiter.next_key_bytes().unwrap(), None);
    jiter.finish().unwrap();
}

#[test]
fn jiter_number() {
    let mut jiter = Jiter::new(br"  [1, 2.2, 3, 4.1, 5.67]");
    assert_eq!(jiter.next_array().unwrap().unwrap().into_inner(), b'1');
    assert_eq!(jiter.next_int().unwrap(), NumberInt::Int(1));
    assert_eq!(jiter.array_step().unwrap().unwrap().into_inner(), b'2');
    assert_eq!(jiter.next_float().unwrap(), 2.2);
    assert_eq!(jiter.array_step().unwrap().unwrap().into_inner(), b'3');

    let n = jiter.next_number().unwrap();
    assert_eq!(n, NumberAny::Int(NumberInt::Int(3)));
    let n_float: f64 = n.into();
    assert_eq!(n_float, 3.0);

    assert_eq!(jiter.array_step().unwrap().unwrap().into_inner(), b'4');
    assert_eq!(jiter.next_number().unwrap(), NumberAny::Float(4.1));
    assert_eq!(jiter.array_step().unwrap().unwrap().into_inner(), b'5');
    assert_eq!(jiter.next_number_bytes().unwrap(), b"5.67");
    assert_eq!(jiter.array_step().unwrap(), None);
    jiter.finish().unwrap();
}

#[test]
fn jiter_bytes_u_escape() {
    let mut jiter = Jiter::new(br#"{"foo": "xx \u00a3"}"#);
    assert_eq!(jiter.next_object_bytes().unwrap().unwrap(), b"foo");
    assert_eq!(jiter.next_bytes().unwrap(), b"xx \\u00a3");

    assert_eq!(jiter.next_key_bytes().unwrap(), None);

    jiter.finish().unwrap();
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
    assert_eq!(jiter.next_array().unwrap().unwrap().into_inner(), b'1');
    assert_eq!(jiter.next_int().unwrap(), NumberInt::Int(1));
    assert!(jiter.array_step().unwrap().is_none());
    let e = jiter.finish().unwrap_err();
    assert_eq!(
        e.error_type,
        JiterErrorType::JsonError(JsonErrorType::TrailingCharacters)
    );
    assert_eq!(jiter.error_position(e.index), LinePosition::new(1, 4));
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
    assert_eq!(jiter.error_position(e.index), LinePosition::new(1, 2));
    assert_eq!(e.to_string(), "expected string but found int at index 1");
    assert_eq!(
        e.description(&jiter),
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
fn test_recursion_limit() {
    let json = (0..2000).map(|_| "[").collect::<String>();
    let bytes = json.as_bytes();
    let e = JsonValue::parse(bytes, false).unwrap_err();
    assert_eq!(e.error_type, JsonErrorType::RecursionLimitExceeded);
    assert_eq!(e.index, 201);
    assert_eq!(e.description(bytes), "recursion limit exceeded at line 1 column 202");
}

#[test]
fn test_recursion_limit_incr() {
    let json = (0..2000).map(|_| "[1]".to_string()).collect::<Vec<_>>().join(", ");
    let json = format!("[{json}]");
    let bytes = json.as_bytes();
    let value = JsonValue::parse(bytes, false).unwrap();
    match value {
        JsonValue::Array(v) => {
            assert_eq!(v.len(), 2000);
        }
        _ => panic!("expected array"),
    }
}

#[test]
fn test_recursion_limit_skip_array() {
    let json = (0..2000).map(|_| "[ ").collect::<String>();
    let bytes = json.as_bytes();
    let mut jiter = Jiter::new(bytes);
    let e = jiter.next_skip().unwrap_err();
    assert_eq!(
        e.error_type,
        JiterErrorType::JsonError(JsonErrorType::RecursionLimitExceeded)
    );
    let expected_index = JsonValue::parse(bytes, false).unwrap_err().index;
    assert_eq!(e.index, expected_index);
    assert_eq!(
        e.description(&jiter),
        format!("recursion limit exceeded at line 1 column {}", expected_index + 1)
    );
}

#[test]
fn test_recursion_limit_skip_object() {
    let json = (0..2000).map(|_| "{\"a\": ").collect::<String>();
    let bytes = json.as_bytes();
    let mut jiter = Jiter::new(bytes);
    let e = jiter.next_skip().unwrap_err();
    assert_eq!(
        e.error_type,
        JiterErrorType::JsonError(JsonErrorType::RecursionLimitExceeded)
    );
    let expected_index = JsonValue::parse(bytes, false).unwrap_err().index;
    assert_eq!(e.index, expected_index);
    assert_eq!(
        e.description(&jiter),
        format!("recursion limit exceeded at line 1 column {}", expected_index + 1)
    );
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

#[cfg(feature = "num-bigint")]
#[test]
fn test_4300_int() {
    let json = (0..4300).map(|_| "9".to_string()).collect::<String>();
    let bytes = json.as_bytes();
    let value = JsonValue::parse(bytes, false).unwrap();
    let expected_big_int = BigInt::from_str(&json).unwrap();
    match value {
        JsonValue::BigInt(v) => {
            assert_eq!(v, expected_big_int);
        }
        _ => panic!("expected array, got {value:?}"),
    }
}

#[cfg(feature = "num-bigint")]
#[test]
fn test_big_int_errs() {
    for json in [
        &[b'9'; 4302][..],
        &[b'9'; 5900][..],
        // If the check is only done at the end, this will hang
        &vec![b'9'; 10usize.pow(7)],
    ] {
        let e = JsonValue::parse(json, false).unwrap_err();
        assert_eq!(e.error_type, JsonErrorType::NumberOutOfRange);
        assert_eq!(e.index, 4301);
        assert_eq!(e.description(json), "number out of range at line 1 column 4302");
    }
}

#[test]
fn readme_jiter() {
    let json_data = r#"
        {
            "name": "John Doe",
            "age": 43,
            "phones": [
                "+44 1234567",
                "+44 2345678"
            ]
        }"#;
    let mut jiter = Jiter::new(json_data.as_bytes());
    assert_eq!(jiter.next_object().unwrap(), Some("name"));
    assert_eq!(jiter.next_str().unwrap(), "John Doe");
    assert_eq!(jiter.next_key().unwrap(), Some("age"));
    assert_eq!(jiter.next_int().unwrap(), NumberInt::Int(43));
    assert_eq!(jiter.next_key().unwrap(), Some("phones"));
    assert_eq!(jiter.next_array().unwrap(), Some(Peek::String));
    // we know the next value is a string as we just asserted so
    assert_eq!(jiter.known_str().unwrap(), "+44 1234567");
    assert_eq!(jiter.array_step().unwrap(), Some(Peek::String));
    // same again
    assert_eq!(jiter.known_str().unwrap(), "+44 2345678");
    // next we'll get `None` from `array_step` as the array is finished
    assert_eq!(jiter.array_step().unwrap(), None);
    // and `None` from `next_key` as the object is finished
    assert_eq!(jiter.next_key().unwrap(), None);
    // and we check there's nothing else in the input
    jiter.finish().unwrap();
}

#[test]
fn jiter_clone() {
    let json = r"[1, 2]";
    let mut jiter1 = Jiter::new(json.as_bytes());
    assert_eq!(jiter1.next_array().unwrap().unwrap().into_inner(), b'1');
    let n = jiter1.next_number().unwrap();
    assert_eq!(n, NumberAny::Int(NumberInt::Int(1)));

    let mut jiter2 = jiter1.clone();

    assert_eq!(jiter1.array_step().unwrap().unwrap().into_inner(), b'2');
    let n = jiter1.next_number().unwrap();
    assert_eq!(n, NumberAny::Int(NumberInt::Int(2)));

    assert_eq!(jiter2.array_step().unwrap().unwrap().into_inner(), b'2');
    let n = jiter2.next_number().unwrap();
    assert_eq!(n, NumberAny::Int(NumberInt::Int(2)));

    assert_eq!(jiter1.array_step().unwrap(), None);
    assert_eq!(jiter2.array_step().unwrap(), None);

    jiter1.finish().unwrap();
    jiter2.finish().unwrap();
}

#[test]
fn jiter_invalid_value() {
    let mut jiter = Jiter::new(b" bar");
    let e = jiter.next_value().unwrap_err();
    assert_eq!(
        e.error_type,
        JiterErrorType::JsonError(JsonErrorType::ExpectedSomeValue)
    );
    assert_eq!(e.index, 1);
    assert_eq!(jiter.error_position(e.index), LinePosition::new(1, 2));
}

#[test]
fn jiter_wrong_types() {
    macro_rules! expect_wrong_type_inner {
        ($actual:path, $input:expr, $method: ident, $expected:path) => {
            let mut jiter = Jiter::new($input);
            let result = jiter.$method();
            if $actual == $expected || matches!(($actual, $expected), (JsonType::Int, JsonType::Float)) {
                // Type matches, or int input to float
                assert!(result.is_ok());
            } else {
                let e = result.unwrap_err();
                assert_eq!(
                    e.error_type,
                    JiterErrorType::WrongType {
                        expected: $expected,
                        actual: $actual,
                    }
                );
            }
        };
    }

    macro_rules! expect_wrong_type {
        ($method:ident, $expected:path) => {
            expect_wrong_type_inner!(JsonType::Array, b"[]", $method, $expected);
            expect_wrong_type_inner!(JsonType::Bool, b"true", $method, $expected);
            expect_wrong_type_inner!(JsonType::Int, b"123", $method, $expected);
            expect_wrong_type_inner!(JsonType::Float, b"123.123", $method, $expected);
            expect_wrong_type_inner!(JsonType::Null, b"null", $method, $expected);
            expect_wrong_type_inner!(JsonType::Object, b"{}", $method, $expected);
            expect_wrong_type_inner!(JsonType::String, b"\"hello\"", $method, $expected);
        };
    }

    expect_wrong_type!(next_array, JsonType::Array);
    expect_wrong_type!(next_bool, JsonType::Bool);
    expect_wrong_type!(next_bytes, JsonType::String);
    expect_wrong_type!(next_null, JsonType::Null);
    expect_wrong_type!(next_object, JsonType::Object);
    expect_wrong_type!(next_object_bytes, JsonType::Object);
    expect_wrong_type!(next_str, JsonType::String);
    expect_wrong_type!(next_int, JsonType::Int);
    expect_wrong_type!(next_float, JsonType::Float);
}

#[test]
fn peek_debug() {
    assert_eq!(format!("{:?}", Peek::True), "True");
    assert_eq!(format!("{:?}", Peek::False), "False");
    assert_eq!(format!("{:?}", Peek::new(b'4')), "Peek('4')");
}

#[test]
fn jiter_invalid_numbers() {
    let mut jiter = Jiter::new(b" -a");
    let peek = jiter.peek().unwrap();
    let e = jiter.known_int(peek).unwrap_err();
    assert_eq!(e.error_type, JiterErrorType::JsonError(JsonErrorType::InvalidNumber));
    let e = jiter.known_float(peek).unwrap_err();
    assert_eq!(e.error_type, JiterErrorType::JsonError(JsonErrorType::InvalidNumber));
    let e = jiter.known_number(peek).unwrap_err();
    assert_eq!(e.error_type, JiterErrorType::JsonError(JsonErrorType::InvalidNumber));
    let e = jiter.next_number_bytes().unwrap_err();
    assert_eq!(e.error_type, JiterErrorType::JsonError(JsonErrorType::InvalidNumber));
}

#[test]
fn jiter_invalid_numbers_expected_some_value() {
    let mut jiter = Jiter::new(b" bar");
    let peek = jiter.peek().unwrap();
    let e = jiter.known_int(peek).unwrap_err();
    assert_eq!(
        e.error_type,
        JiterErrorType::JsonError(JsonErrorType::ExpectedSomeValue)
    );
    let e = jiter.known_float(peek).unwrap_err();
    assert_eq!(
        e.error_type,
        JiterErrorType::JsonError(JsonErrorType::ExpectedSomeValue)
    );
    let e = jiter.known_number(peek).unwrap_err();
    assert_eq!(
        e.error_type,
        JiterErrorType::JsonError(JsonErrorType::ExpectedSomeValue)
    );
    let e = jiter.next_number_bytes().unwrap_err();
    assert_eq!(
        e.error_type,
        JiterErrorType::JsonError(JsonErrorType::ExpectedSomeValue)
    );
}

fn value_owned() -> JsonValue<'static> {
    let s = r#"  { "int": 1, "const": true, "float": 1.2, "array": [1, false, null]}"#.to_string();
    JsonValue::parse_owned(s.as_bytes(), false, PartialMode::Off).unwrap()
}

fn get_key<'a, 'j>(o: &'a JsonObject<'j>, key: &str) -> Option<&'a JsonValue<'j>> {
    o.iter().find_map(|(k, v)| (k == key).then_some(v))
}

#[test]
fn test_owned_value() {
    let value = value_owned();
    let JsonValue::Object(obj) = value else {
        panic!("expected object")
    };
    assert_eq!(get_key(&obj, "int").unwrap(), &JsonValue::Int(1));
    assert_eq!(get_key(&obj, "const").unwrap(), &JsonValue::Bool(true));
    assert_eq!(get_key(&obj, "float").unwrap(), &JsonValue::Float(1.2));
    let JsonValue::Array(array) = get_key(&obj, "array").unwrap() else {
        panic!("expected array")
    };
    assert_eq!(
        array,
        &Arc::new(vec![JsonValue::Int(1), JsonValue::Bool(false), JsonValue::Null])
    );
}

fn value_into_static() -> JsonValue<'static> {
    let s = r#"{ "big_int": 92233720368547758070, "const": true, "float": 1.2, "array": [1, false, null, "x"]}"#
        .to_string();
    let jiter = JsonValue::parse(s.as_bytes(), false).unwrap();
    jiter.into_static()
}

#[cfg(feature = "num-bigint")]
#[test]
fn test_into_static() {
    let value = crate::value_into_static();
    let JsonValue::Object(obj) = value else {
        panic!("expected object")
    };
    let expected_big_int = BigInt::from_str("92233720368547758070").unwrap();
    assert_eq!(get_key(&obj, "big_int").unwrap(), &JsonValue::BigInt(expected_big_int));
    assert_eq!(get_key(&obj, "const").unwrap(), &JsonValue::Bool(true));
    assert_eq!(get_key(&obj, "float").unwrap(), &JsonValue::Float(1.2));
    let JsonValue::Array(array) = get_key(&obj, "array").unwrap() else {
        panic!("expected array")
    };
    assert_eq!(
        array,
        &Arc::new(vec![
            JsonValue::Int(1),
            JsonValue::Bool(false),
            JsonValue::Null,
            JsonValue::Str("x".into())
        ])
    );
}

#[test]
fn jiter_next_value_borrowed() {
    let mut jiter = Jiter::new(br#" "v"  "#);
    let v = jiter.next_value().unwrap();
    let JsonValue::Str(s) = v else {
        panic!("expected string")
    };
    assert_eq!(s, "v");
    assert!(matches!(s, Cow::Borrowed(_)));
}

#[test]
fn jiter_next_value_owned() {
    let mut jiter = Jiter::new(br#" "v"  "#);
    let v = jiter.next_value_owned().unwrap();
    let JsonValue::Str(s) = v else {
        panic!("expected string")
    };
    assert_eq!(s, "v");
    assert!(matches!(s, Cow::Owned(_)));
}

#[cfg(feature = "num-bigint")]
#[test]
fn i64_max() {
    let json = "9223372036854775807";
    assert_eq!(i64::MAX.to_string(), json);
    let v = JsonValue::parse(json.as_bytes(), false).unwrap();
    match v {
        JsonValue::Int(v) => assert_eq!(v, i64::MAX),
        JsonValue::BigInt(v) => assert_eq!(v, i64::MAX.into()),
        _ => panic!("expected int"),
    }
}

#[cfg(feature = "num-bigint")]
#[test]
fn test_all_int_lengths() {
    for int_size in 1..100 {
        let json = "9".repeat(int_size);
        let v = JsonValue::parse(json.as_bytes(), false).unwrap();
        match v {
            JsonValue::Int(v) => assert_eq!(v.to_string(), json),
            JsonValue::BigInt(v) => assert_eq!(v.to_string(), json),
            _ => panic!("expected int"),
        }
    }
}

#[test]
fn test_number_int_try_from_bytes() {
    let n: NumberInt = b"123".as_ref().try_into().unwrap();
    assert_eq!(n, NumberInt::Int(123));

    let n: NumberInt = b"0".as_ref().try_into().unwrap();
    assert_eq!(n, NumberInt::Int(0));

    #[cfg(feature = "num-bigint")]
    {
        let twenty_nines = "9".repeat(29);
        let n: NumberInt = twenty_nines.as_bytes().try_into().unwrap();
        match n {
            NumberInt::BigInt(v) => assert_eq!(v.to_string(), twenty_nines),
            NumberInt::Int(_) => panic!("expected big int"),
        }
    }

    let e = NumberInt::try_from(b"x23".as_ref()).unwrap_err();
    assert_eq!(e.to_string(), "invalid number at index 0");

    let e = NumberInt::try_from(b"".as_ref()).unwrap_err();
    assert_eq!(e.to_string(), "invalid number at index 0");

    let e = NumberInt::try_from(b"2x3".as_ref()).unwrap_err();
    assert_eq!(e.to_string(), "invalid number at index 1");

    let e = NumberInt::try_from(b"123 ".as_ref()).unwrap_err();
    assert_eq!(e.to_string(), "invalid number at index 3");

    let e = NumberInt::try_from(b"123.1".as_ref()).unwrap_err();
    assert_eq!(e.to_string(), "invalid number at index 3");

    let e = NumberInt::try_from(b"0123".as_ref()).unwrap_err();
    assert_eq!(e.to_string(), "invalid number at index 1");

    #[cfg(feature = "num-bigint")]
    {
        let too_long = "9".repeat(4309);
        let e = NumberInt::try_from(too_long.as_bytes()).unwrap_err();
        assert_eq!(e.to_string(), "number out of range at index 4301");
    }
}

#[test]
fn jiter_skip_whole_object() {
    let mut jiter = Jiter::new(br#"{"x": 1}"#);
    jiter.next_skip().unwrap();
    jiter.finish().unwrap();
}

#[test]
fn jiter_skip_in_object() {
    let mut jiter = Jiter::new(
        br#" {
        "is_bool": true,
        "is_int": 123,
        "is_float": 123.456,
        "is_str": "x",
        "is_array": [0, 1, 2, 3, "4", [],  {}],
        "is_object": {"x": 1, "y": ["2"], "z": {}},
        "last": 123
     } "#,
    );

    assert_eq!(jiter.next_object(), Ok(Some("is_bool")));
    jiter.next_skip().unwrap();

    assert_eq!(jiter.next_key(), Ok(Some("is_int")));
    jiter.next_skip().unwrap();

    assert_eq!(jiter.next_key(), Ok(Some("is_float")));
    jiter.next_skip().unwrap();

    assert_eq!(jiter.next_key(), Ok(Some("is_str")));
    jiter.next_skip().unwrap();

    assert_eq!(jiter.next_key(), Ok(Some("is_array")));
    let peek = jiter.peek().unwrap();
    let start = jiter.current_index();
    jiter.known_skip(peek).unwrap();
    let array_slice = jiter.slice_to_current(start);
    assert_eq!(array_slice, br#"[0, 1, 2, 3, "4", [],  {}]"#);

    assert_eq!(jiter.next_key(), Ok(Some("is_object")));
    jiter.next_skip().unwrap();

    assert_eq!(jiter.next_key(), Ok(Some("last")));
    assert_eq!(jiter.next_int(), Ok(NumberInt::Int(123)));
    assert_eq!(jiter.next_key().unwrap(), None);

    jiter.finish().unwrap();
}

#[test]
fn jiter_skip_in_array() {
    let mut jiter = Jiter::new(
        br#" [
        true,
        false,
        null,
        NaN,
        Infinity,
        -Infinity,
        123,
        234.566,
        345e45,
        "",
        "\u00a3",
        "\"",
        "last item"
     ] "#,
    )
    .with_allow_inf_nan();

    assert_eq!(jiter.next_array(), Ok(Some(Peek::True)));
    jiter.known_skip(Peek::True).unwrap(); // true

    assert_eq!(jiter.array_step(), Ok(Some(Peek::False)));
    jiter.known_skip(Peek::False).unwrap(); // false

    assert_eq!(jiter.array_step(), Ok(Some(Peek::Null)));
    jiter.known_skip(Peek::Null).unwrap(); // null

    assert_eq!(jiter.array_step(), Ok(Some(Peek::NaN)));
    jiter.known_skip(Peek::NaN).unwrap(); // NaN

    assert_eq!(jiter.array_step(), Ok(Some(Peek::Infinity)));
    jiter.known_skip(Peek::Infinity).unwrap(); // Infinity

    assert_eq!(jiter.array_step(), Ok(Some(Peek::Minus)));
    jiter.known_skip(Peek::Minus).unwrap(); // -Infinity

    assert_eq!(jiter.array_step(), Ok(Some(Peek::new(b'1'))));
    jiter.known_skip(Peek::new(b'1')).unwrap(); // 123

    assert_eq!(jiter.array_step(), Ok(Some(Peek::new(b'2'))));
    jiter.known_skip(Peek::new(b'2')).unwrap(); // 234.566

    assert_eq!(jiter.array_step(), Ok(Some(Peek::new(b'3'))));
    jiter.known_skip(Peek::new(b'3')).unwrap(); // 345e45

    assert_eq!(jiter.array_step(), Ok(Some(Peek::String)));
    jiter.known_skip(Peek::String).unwrap(); // ""

    assert_eq!(jiter.array_step(), Ok(Some(Peek::String)));
    jiter.known_skip(Peek::String).unwrap(); // "\u00a3"

    assert_eq!(jiter.array_step(), Ok(Some(Peek::String)));
    jiter.known_skip(Peek::String).unwrap(); // "\""

    assert_eq!(jiter.array_step(), Ok(Some(Peek::String)));
    assert_eq!(jiter.known_str(), Ok("last item"));

    assert_eq!(jiter.array_step(), Ok(None));

    jiter.finish().unwrap();
}

#[test]
fn jiter_skip_backslash_strings() {
    let mut jiter = Jiter::new(br#" ["\"", "\n", "\t", "\u00a3", "\\"] "#);
    jiter.next_skip().unwrap();
    jiter.finish().unwrap();
}

#[test]
fn jiter_skip_invalid_ident() {
    let mut jiter = Jiter::new(br"trUe").with_allow_inf_nan();
    let e = jiter.next_skip().unwrap_err();
    assert_eq!(
        e.error_type,
        JiterErrorType::JsonError(JsonErrorType::ExpectedSomeIdent)
    );
}

#[test]
fn jiter_skip_invalid_string() {
    let mut jiter = Jiter::new(br#" "foo "#).with_allow_inf_nan();
    let e = jiter.next_skip().unwrap_err();
    assert_eq!(
        e.error_type,
        JiterErrorType::JsonError(JsonErrorType::EofWhileParsingString)
    );
}

#[test]
fn jiter_skip_invalid_int() {
    let mut jiter = Jiter::new(br"01");
    let e = jiter.next_skip().unwrap_err();
    assert_eq!(e.error_type, JiterErrorType::JsonError(JsonErrorType::InvalidNumber));
}

#[test]
fn jiter_skip_invalid_object() {
    let mut jiter = Jiter::new(br"{{");
    let e = jiter.next_skip().unwrap_err();
    assert_eq!(e.error_type, JiterErrorType::JsonError(JsonErrorType::KeyMustBeAString));
}

#[test]
fn jiter_skip_invalid_string_u() {
    let mut jiter = Jiter::new(br#" "\uddBd" "#);
    let e = jiter.next_skip().unwrap_err();
    assert_eq!(
        e.error_type,
        JiterErrorType::JsonError(JsonErrorType::LoneLeadingSurrogateInHexEscape)
    );
}

#[test]
fn jiter_skip_invalid_nan() {
    let mut jiter = Jiter::new(b"NaN");
    let e = jiter.next_skip().unwrap_err();
    assert_eq!(
        e.error_type,
        JiterErrorType::JsonError(JsonErrorType::ExpectedSomeValue)
    );
}

#[test]
fn jiter_skip_invalid_string_high() {
    let json = vec![34, 92, 34, 206, 44, 163, 34];
    let mut jiter = Jiter::new(&json);
    // NOTE this would raise an error with next_value etc, but next_skip does not check UTF-8
    jiter.next_skip().unwrap();
    jiter.finish().unwrap();
}

#[test]
fn jiter_skip_invalid_long_float() {
    let mut jiter = Jiter::new(br"2121515572557277572557277e");
    let e = jiter.next_skip().unwrap_err();
    assert_eq!(
        e.error_type,
        JiterErrorType::JsonError(JsonErrorType::EofWhileParsingValue)
    );
}

#[cfg(feature = "num-bigint")]
#[test]
fn jiter_value_invalid_long_float() {
    let e = JsonValue::parse(br"2121515572557277572557277e", false).unwrap_err();
    assert_eq!(e.error_type, JsonErrorType::EofWhileParsingValue);
}

#[test]
fn jiter_partial_string() {
    let mut jiter = Jiter::new(br#"["foo"#).with_allow_partial_strings();
    assert_eq!(jiter.next_array().unwrap(), Some(Peek::String));
    assert_eq!(jiter.next_str().unwrap(), "foo");
    let e = jiter.array_step().unwrap_err();
    assert_eq!(
        e.error_type,
        JiterErrorType::JsonError(JsonErrorType::EofWhileParsingList)
    );
}

#[test]
fn jiter_partial_string_escape() {
    let mut jiter = Jiter::new(br#""foo\"#).with_allow_partial_strings();
    assert_eq!(jiter.next_str().unwrap(), "foo");

    let mut jiter = Jiter::new(br#""foo\u"#).with_allow_partial_strings();
    assert_eq!(jiter.next_str().unwrap(), "foo");

    let mut jiter = Jiter::new(br#""foo\u1"#).with_allow_partial_strings();
    assert_eq!(jiter.next_str().unwrap(), "foo");

    let mut jiter = Jiter::new(br#""foo\u12"#).with_allow_partial_strings();
    assert_eq!(jiter.next_str().unwrap(), "foo");

    let mut jiter = Jiter::new(br#""foo\u123"#).with_allow_partial_strings();
    assert_eq!(jiter.next_str().unwrap(), "foo");
}

#[test]
fn test_unicode_roundtrip() {
    // '"中文"'
    let json_bytes = b"\"\\u4e2d\\u6587\"";
    let value = JsonValue::parse(json_bytes, false).unwrap();
    let JsonValue::Str(cow) = value else {
        panic!("expected string")
    };
    assert_eq!(cow, "中文");
    assert!(matches!(cow, Cow::Owned(_)));
}

#[test]
fn test_value_partial_array_on() {
    let json_bytes = br#"["string", true, null, 1, "foo"#;
    let value = JsonValue::parse_with_config(json_bytes, false, PartialMode::On).unwrap();
    assert_eq!(
        value,
        JsonValue::Array(Arc::new(vec![
            JsonValue::Str("string".into()),
            JsonValue::Bool(true),
            JsonValue::Null,
            JsonValue::Int(1),
        ]))
    );
    // test all position in the string
    for i in 1..json_bytes.len() {
        let partial_json = &json_bytes[..i];
        let value = JsonValue::parse_with_config(partial_json, false, PartialMode::On).unwrap();
        assert!(matches!(value, JsonValue::Array(_)));
    }
}

#[test]
fn test_value_partial_array_trailing_strings() {
    let json_bytes = br#"["string", true, null, 1, "foo"#;
    let value = JsonValue::parse_with_config(json_bytes, false, PartialMode::TrailingStrings).unwrap();
    assert_eq!(
        value,
        JsonValue::Array(Arc::new(vec![
            JsonValue::Str("string".into()),
            JsonValue::Bool(true),
            JsonValue::Null,
            JsonValue::Int(1),
            JsonValue::Str("foo".into()),
        ]))
    );
    // test all position in the string
    for i in 1..json_bytes.len() {
        let partial_json = &json_bytes[..i];
        let value = JsonValue::parse_with_config(partial_json, false, PartialMode::TrailingStrings).unwrap();
        assert!(matches!(value, JsonValue::Array(_)));
    }
}

#[test]
fn test_value_partial_object() {
    let json_bytes = br#"{"a": "value", "b": true, "c": false, "d": null, "e": 1, "f": 2.22, "g": ["#;
    let value = JsonValue::parse_with_config(json_bytes, false, PartialMode::TrailingStrings).unwrap();
    let JsonValue::Object(obj) = value else {
        panic!("expected object")
    };
    assert_eq!(obj.len(), 7);
    let pairs = obj.iter().collect::<Vec<_>>();
    assert_eq!(pairs[0].clone(), (Cow::Borrowed("a"), JsonValue::Str("value".into())));
    assert_eq!(pairs[1].clone(), (Cow::Borrowed("b"), JsonValue::Bool(true)));
    assert_eq!(pairs[2].clone(), (Cow::Borrowed("c"), JsonValue::Bool(false)));
    assert_eq!(pairs[3].clone(), (Cow::Borrowed("d"), JsonValue::Null));
    assert_eq!(pairs[4].clone(), (Cow::Borrowed("e"), JsonValue::Int(1)));
    assert_eq!(pairs[5].clone(), (Cow::Borrowed("f"), JsonValue::Float(2.22)));
    assert_eq!(
        pairs[6].clone(),
        (Cow::Borrowed("g"), JsonValue::Array(Arc::new(vec![])))
    );
    // test all position in the string
    for i in 1..json_bytes.len() {
        let partial_json = &json_bytes[..i];
        let value = JsonValue::parse_with_config(partial_json, false, PartialMode::TrailingStrings).unwrap();
        assert!(matches!(value, JsonValue::Object(_)));
    }
}

#[test]
fn test_partial_pass1() {
    let json = read_file("./benches/pass1.json");
    let json_bytes = json.as_bytes();

    // test all position in the string
    for i in 1..json_bytes.len() {
        let partial_json = &json_bytes[..i];
        let value = JsonValue::parse_with_config(partial_json, false, PartialMode::TrailingStrings).unwrap();
        assert!(matches!(value, JsonValue::Array(_)));
    }
}

#[test]
fn test_partial_medium_response() {
    let json = read_file("./benches/medium_response.json");
    let json_bytes = json.as_bytes();

    // test all position in the string
    for i in 1..json_bytes.len() {
        let partial_json = &json_bytes[..i];
        let value = JsonValue::parse_with_config(partial_json, false, PartialMode::TrailingStrings).unwrap();
        assert!(matches!(value, JsonValue::Object(_)));
    }
}
