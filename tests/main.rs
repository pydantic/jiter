use indexmap::indexmap;
use std::fs::File;
use std::io::Read;

use jiter::{
    FilePosition, Jiter, JiterError, JsonError, JsonResult, JsonType, JsonValue, NumberAny, NumberDecoder, NumberInt,
    Parser, Peak, StringDecoder, StringDecoderRange,
};

fn json_vec(parser: &mut Parser) -> JsonResult<Vec<String>> {
    let mut v = Vec::new();
    let peak = parser.peak()?;
    let position = parser.current_position();
    match peak {
        Peak::True => {
            parser.consume_true()?;
            dbg!("true");
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
            let range = parser.consume_string::<StringDecoderRange>()?;
            v.push(format!("String({range:?}) @ {position}"));
        }
        Peak::Num(positive) => {
            let s = display_number(positive, parser)?;
            v.push(s);
        }
        Peak::Array => {
            v.push(format!("[ @ {position}"));
            if parser.array_first()? {
                loop {
                    let el_vec = json_vec(parser)?;
                    v.extend(el_vec);
                    if !parser.array_step()? {
                        break;
                    }
                }
            }
            v.push("]".to_string());
        }
        Peak::Object => {
            v.push(format!("{{ @ {position}"));
            if let Some(key) = parser.object_first::<StringDecoderRange>()? {
                v.push(format!("Key({key:?})"));
                let value_vec = json_vec(parser)?;
                v.extend(value_vec);
                while let Some(key) = parser.object_step::<StringDecoderRange>()? {
                    v.push(format!("Key({key:?}"));
                    let value_vec = json_vec(parser)?;
                    v.extend(value_vec);
                }
            }
            v.push("}".to_string());
        }
    };
    Ok(v)
}

fn display_number(positive: bool, parser: &mut Parser) -> JsonResult<String> {
    let position = parser.current_position();
    let number = parser.consume_number::<NumberDecoder<NumberAny>>(positive)?;
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
            #[test]
            fn [< single_element_ok_ $name >]() {
                let mut parser = Parser::new($json.as_bytes());
                let elements = json_vec(&mut parser).unwrap().join(", ");
                assert_eq!(elements, $expected);
                parser.finish().unwrap();
            }
        }
    };
    ($name:ident, err, $json:literal, $error:expr) => {
        paste::item! {
            #[test]
            fn [< single_element_xerror_ $name _ $error:snake _error >]() {
                let result = json_vec(&mut Parser::new($json.as_bytes()));
                match result {
                    Ok(t) => panic!("unexpectedly valid: {:?} -> {:?}", $json, t),
                    Err(e) => assert_eq!(e, JsonError::$error),
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
    int_pos: ok => "1234", "Int(1234) @ 1:1";
    int_neg: ok => "-1234", "Int(-1234) @ 1:1";
    int_exp: ok => "20e10", "Float(200000000000) @ 1:1";
    float_pos: ok => "12.34", "Float(12.34) @ 1:1";
    float_neg: ok => "-12.34", "Float(-12.34) @ 1:1";
    float_exp: ok => "2.2e10", "Float(22000000000) @ 1:1";
    float_exp_pos: ok => "2.2e+10", "Float(22000000000) @ 1:1";
    // NOTICE - this might be brittle, if so move to to a separate test
    float_exp_neg: ok => "2.2e-2", "Float(0.022000000000000002) @ 1:1";
    float_exp_massive1: ok => "2e2147483647", "Float(inf) @ 1:1";
    float_exp_massive2: ok => "2e2147483648", "Float(inf) @ 1:1";
    float_exp_massive3: ok => "2e2147483646", "Float(inf) @ 1:1";
    float_exp_tiny0: ok => "2e-2147483647", "Float(0) @ 1:1";
    float_exp_tiny1: ok => "2e-2147483648", "Float(0) @ 1:1";
    float_exp_tiny2: ok => "2e-2147483646", "Float(0) @ 1:1";
    float_exp_tiny3: ok => "8e-7766666666", "Float(0) @ 1:1";
    // float_exp_tiny4: ok => "200.08e-76200000102", "Float(0) @ 1:1";
    null: ok => "null", "null @ 1:1";
    v_true: ok => "true", "true @ 1:1";
    v_false: ok => "false", "false @ 1:1";
    offset_true: ok => "  true", "true @ 1:3";
    string_unclosed: err => r#""foobar"#, UnexpectedEnd;
    bad_int: err => "-", UnexpectedEnd;
    bad_true: err => "truX", InvalidTrue;
    bad_true: err => "tru", UnexpectedEnd;
    bad_false: err => "falsX", InvalidFalse;
    bad_false: err => "fals", UnexpectedEnd;
    bad_null: err => "nulX", InvalidNull;
    bad_null: err => "nul", UnexpectedEnd;
    object_trailing_comma: err => r#"{"foo": "bar",}"#, UnexpectedCharacter;
    array_trailing_comma: err => r#"[1, 2,]"#, UnexpectedCharacter;
    array_bool: ok => "[true, false]", "[ @ 1:1, true @ 1:2, false @ 1:8, ]";
    object_string: ok => r#"{"foo": "ba"}"#, "{ @ 1:1, Key(2..5), String(9..11) @ 1:9, }";
    object_null: ok => r#"{"foo": null}"#, "{ @ 1:1, Key(2..5), null @ 1:9, }";
    object_bool_compact: ok => r#"{"foo":true}"#, "{ @ 1:1, Key(2..5), true @ 1:8, }";
    deep_array: ok => r#"[["Not too deep"]]"#, "[ @ 1:1, [ @ 1:2, String(3..15) @ 1:3, ], ]";
    object_key_int: err => r#"{4: 4}"#, UnexpectedCharacter;
    array_no_close: err => r#"["#, UnexpectedEnd;
    // array_double_close: err => r#"[1]]"#, UnexpectedCharacter;
}

#[test]
fn invalid_string_controls() {
    let json = "\"123\x08\x0c\n\r\t\"";
    let b = json.as_bytes();
    let mut parser = Parser::new(b);
    let peak = parser.peak().unwrap();
    assert!(matches!(peak, Peak::String));
    let result = parser.consume_string::<StringDecoder>();
    match result {
        Ok(t) => panic!("unexpectedly valid: {:?} -> {:?}", json, t),
        Err(e) => assert_eq!(e, JsonError::InvalidString(3)),
    }
}

#[test]
fn json_parse_str() {
    let json = r#" "foobar" "#;
    let data = json.as_bytes();
    let mut parser = Parser::new(data);
    let peak = parser.peak().unwrap();
    assert!(matches!(peak, Peak::String));
    assert_eq!(parser.current_position(), FilePosition::new(1, 2));

    let result_string = parser.consume_string::<StringDecoder>().unwrap();
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
                    let mut parser = Parser::new(data);
                    let peak = parser.peak().unwrap();
                    assert!(matches!(peak, Peak::String));
                    let result_string = parser.consume_string::<StringDecoder>().unwrap();
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
    backslash: r#""\\""# => r#"\"#;
    controls: "\"\\b\\f\\n\\r\\t\"" => "\u{8}\u{c}\n\r\t";
    controls_python: "\"\\b\\f\\n\\r\\t\"" => "\x08\x0c\n\r\t";  // python notation for the same thing
}

#[test]
fn test_key_str() {
    let json = r#"{"foo": "bar"}"#;
    let mut parser = Parser::new(json.as_bytes());
    let p = parser.peak().unwrap();
    assert!(matches!(p, Peak::Object));
    let k = parser.object_first::<StringDecoder>().unwrap();
    assert_eq!(k, Some("foo".to_string()));
    let p = parser.peak().unwrap();
    assert!(matches!(p, Peak::String));
    let v = parser.consume_string::<StringDecoder>().unwrap();
    assert_eq!(v, "bar");
    let next_key = parser.object_step::<StringDecoder>().unwrap();
    assert!(next_key.is_none());
    parser.finish().unwrap();
}

#[test]
fn test_key_bytes() {
    let json = r#"{"foo": "bar"}"#.as_bytes();
    let mut parser = Parser::new(json);
    let p = parser.peak().unwrap();
    assert!(matches!(p, Peak::Object));
    let k = parser.object_first::<StringDecoderRange>().unwrap().unwrap();
    assert_eq!(json[k], *b"foo");
    let p = parser.peak().unwrap();
    assert!(matches!(p, Peak::String));
    let v = parser.consume_string::<StringDecoderRange>().unwrap();
    assert_eq!(json[v], *b"bar");
    let next_key = parser.object_step::<StringDecoder>().unwrap();
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
    first_line_zero: b"123456", 0, FilePosition::new(1, 1);
    first_line_first: b"123456", 1, FilePosition::new(1, 2);
    first_line_3rd: b"123456", 3, FilePosition::new(1, 4);
    first_line_last: b"123456", 6, FilePosition::new(1, 7);
    first_line_after: b"123456", 7, FilePosition::new(1, 7);
    first_line_last2: b"123456\n789", 6, FilePosition::new(1, 7);
    second_line: b"123456\n789", 7, FilePosition::new(2, 1);
}

#[test]
fn parse_int() {
    let v = JsonValue::parse(b"8e-7766666666").unwrap();
    assert_eq!(v, JsonValue::Float(0.0));
}

#[test]
fn parse_object() {
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
fn repeat_trailing_array() {
    let json = "[1]]";
    let result = JsonValue::parse(json.as_bytes());
    match result {
        Ok(t) => panic!("unexpectedly valid: {:?} -> {:?}", json, t),
        Err(e) => {
            assert_eq!(e.error, JsonError::UnexpectedCharacter);
            assert_eq!(e.position, FilePosition::new(1, 4));
        }
    }
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

#[test]
fn jiter_object() {
    let mut jiter = Jiter::new(br#"{"foo": "bar", "spam": [   1, 2, "x"]}"#);
    assert_eq!(jiter.next_object().unwrap(), Some("foo".to_string()));
    assert_eq!(jiter.next_str().unwrap(), "bar");
    assert_eq!(jiter.next_key().unwrap(), Some("spam".to_string()));
    assert_eq!(jiter.next_array().unwrap(), true);
    assert_eq!(jiter.next_int().unwrap(), NumberInt::Int(1));
    assert_eq!(jiter.array_step().unwrap(), true);
    assert_eq!(jiter.next_int().unwrap(), NumberInt::Int(2));
    assert_eq!(jiter.array_step().unwrap(), true);
    assert_eq!(jiter.next_bytes().unwrap(), b"x");
    assert_eq!(jiter.array_step().unwrap(), false);
    assert_eq!(jiter.next_key().unwrap(), None);
    jiter.finish().unwrap();
}

#[test]
fn jiter_empty_array() {
    let mut jiter = Jiter::new(b"[]");
    assert_eq!(jiter.next_array().unwrap(), false);
    jiter.finish().unwrap();
}

#[test]
fn jiter_trailing_bracket() {
    let mut jiter = Jiter::new(b"[1]]");
    assert_eq!(jiter.next_array().unwrap(), true);
    assert_eq!(jiter.next_int().unwrap(), NumberInt::Int(1));
    assert_eq!(jiter.array_step().unwrap(), false);
    let result = jiter.finish();
    match result {
        Ok(t) => panic!("unexpectedly valid: {:?}", t),
        Err(JiterError::JsonError { error, position }) => {
            assert_eq!(error, JsonError::UnexpectedCharacter);
            assert_eq!(position, FilePosition::new(1, 4));
        }
        Err(other_err) => panic!("unexpected error: {:?}", other_err),
    }
}

#[test]
fn jiter_wrong_type() {
    let mut jiter = Jiter::new(b" 123");
    let result = jiter.next_str();
    match result {
        Ok(t) => panic!("unexpectedly valid: {:?}", t),
        Err(JiterError::WrongType {
            expected,
            actual,
            position,
        }) => {
            assert_eq!(expected, JsonType::String);
            assert_eq!(actual, JsonType::Int);
            assert_eq!(position, FilePosition::new(1, 2));
        }
        Err(other_err) => panic!("unexpected error: {:?}", other_err),
    }
}
