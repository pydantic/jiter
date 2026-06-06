use std::collections::BTreeMap;

use serde::Deserialize;

use jiter::serde::{Error, JiterDeserializer, from_slice, from_str};

fn de_f64_inf_nan(s: &str) -> f64 {
    let mut de = JiterDeserializer::new(s.as_bytes()).with_allow_inf_nan();
    let v = f64::deserialize(&mut de).unwrap();
    de.finish().unwrap();
    v
}

fn de_value_inf_nan(s: &str) -> serde_json::Value {
    let mut de = JiterDeserializer::new(s.as_bytes()).with_allow_inf_nan();
    let v = serde_json::Value::deserialize(&mut de).unwrap();
    de.finish().unwrap();
    v
}

#[test]
fn primitives() {
    assert!(from_str::<bool>("true").unwrap());
    assert_eq!(from_str::<i64>("-42").unwrap(), -42);
    assert_eq!(from_str::<u64>("42").unwrap(), 42);
    assert!((from_str::<f64>("2.5").unwrap() - 2.5).abs() < f64::EPSILON);
    assert_eq!(from_str::<String>(r#""hello""#).unwrap(), "hello");
    assert_eq!(from_str::<Option<i64>>("null").unwrap(), None);
    assert_eq!(from_str::<Option<i64>>("5").unwrap(), Some(5));
    assert_eq!(from_str::<()>("null").unwrap(), ());
}

#[test]
fn vecs_and_maps() {
    assert_eq!(from_str::<Vec<i64>>("[1, 2, 3]").unwrap(), vec![1, 2, 3]);
    assert_eq!(from_str::<Vec<i64>>("[]").unwrap(), Vec::<i64>::new());

    let map: BTreeMap<String, i64> = from_str(r#"{"a": 1, "b": 2}"#).unwrap();
    assert_eq!(map, BTreeMap::from([("a".to_string(), 1), ("b".to_string(), 2)]));

    // integer keys (parsed from the key string)
    let map: BTreeMap<u32, String> = from_str(r#"{"1": "one", "2": "two"}"#).unwrap();
    assert_eq!(map, BTreeMap::from([(1, "one".to_string()), (2, "two".to_string())]));
}

#[derive(Deserialize, PartialEq, Debug)]
struct Person<'a> {
    name: &'a str,
    age: u8,
    phones: Vec<&'a str>,
}

#[test]
fn borrowed_struct() {
    let data = r#"{"name": "John Doe", "age": 43, "phones": ["+44 111", "+44 222"]}"#;
    let person: Person = from_str(data).unwrap();
    assert_eq!(
        person,
        Person {
            name: "John Doe",
            age: 43,
            phones: vec!["+44 111", "+44 222"],
        }
    );
}

#[test]
fn zero_copy_borrowed_str() {
    // unescaped `&str` borrows from the input: the slice points inside the input buffer
    let data = br#""no escapes here""#;
    let s: &str = from_slice(data).unwrap();
    assert_eq!(s, "no escapes here");
    let input = data.as_ptr() as usize..(data.as_ptr() as usize + data.len());
    assert!(
        input.contains(&(s.as_ptr() as usize)),
        "string should borrow from input"
    );
}

#[test]
fn escaped_string_is_owned() {
    // escapes force decoding onto the tape; deserializing to an owned `String` works
    let s: String = from_str(r#""tab\tnewline\n""#).unwrap();
    assert_eq!(s, "tab\tnewline\n");
}

#[test]
fn borrowed_str_with_escape_errors() {
    // an escaped `&str` can't be borrowed (same as serde_json)
    let err = from_str::<&str>(r#""has \t escape""#).unwrap_err();
    assert!(matches!(err, Error::Data { .. }));
}

#[derive(Deserialize, PartialEq, Debug)]
struct Nested {
    inner: Inner,
    list: Vec<Inner>,
}

#[derive(Deserialize, PartialEq, Debug)]
struct Inner {
    value: i64,
}

#[test]
fn nested() {
    let data = r#"{"inner": {"value": 1}, "list": [{"value": 2}, {"value": 3}]}"#;
    let nested: Nested = from_str(data).unwrap();
    assert_eq!(
        nested,
        Nested {
            inner: Inner { value: 1 },
            list: vec![Inner { value: 2 }, Inner { value: 3 }],
        }
    );
}

#[derive(Deserialize, PartialEq, Debug)]
enum Shape {
    Unit,
    Newtype(i64),
    Tuple(i64, i64),
    Struct { x: i64, y: i64 },
}

#[test]
fn enums() {
    assert_eq!(from_str::<Shape>(r#""Unit""#).unwrap(), Shape::Unit);
    assert_eq!(from_str::<Shape>(r#"{"Newtype": 7}"#).unwrap(), Shape::Newtype(7));
    assert_eq!(from_str::<Shape>(r#"{"Tuple": [1, 2]}"#).unwrap(), Shape::Tuple(1, 2));
    assert_eq!(
        from_str::<Shape>(r#"{"Struct": {"x": 1, "y": 2}}"#).unwrap(),
        Shape::Struct { x: 1, y: 2 }
    );
}

#[derive(Deserialize, PartialEq, Debug)]
struct Ordered {
    a: i64,
    b: String,
    c: bool,
    #[serde(default)]
    d: Option<i64>,
}

#[test]
fn field_order_independent() {
    let declared = Ordered {
        a: 1,
        b: "two".to_string(),
        c: true,
        d: Some(4),
    };

    // declaration order
    assert_eq!(
        from_str::<Ordered>(r#"{"a": 1, "b": "two", "c": true, "d": 4}"#).unwrap(),
        declared
    );
    // fully reversed order
    assert_eq!(
        from_str::<Ordered>(r#"{"d": 4, "c": true, "b": "two", "a": 1}"#).unwrap(),
        declared
    );
    // shuffled, unknown fields interspersed, `d` omitted
    let shuffled = r#"{"c": true, "x": [1, 2, 3], "b": "two", "y": {"z": null}, "a": 1}"#;
    assert_eq!(
        from_str::<Ordered>(shuffled).unwrap(),
        Ordered {
            a: 1,
            b: "two".to_string(),
            c: true,
            d: None,
        }
    );
}

#[test]
fn skips_unknown_fields() {
    // unknown fields are skipped
    let data = r#"{"value": 5, "unknown": [1, 2, {"deep": "nested"}], "more": null}"#;
    let inner: Inner = from_str(data).unwrap();
    assert_eq!(inner, Inner { value: 5 });
}

#[test]
fn trailing_data_errors() {
    assert!(from_str::<i64>("1 2").is_err());
    assert!(from_str::<Vec<i64>>("[1, 2] extra").is_err());
}

#[test]
fn wrong_type_errors() {
    let err = from_str::<i64>(r#""not a number""#).unwrap_err();
    // string where i64 expected
    assert!(matches!(err, Error::Data { .. }));
}

#[test]
fn type_error_carries_real_position() {
    #[derive(Deserialize, Debug)]
    #[allow(dead_code)]
    struct S {
        a: i64,
        b: i64,
    }
    // the error points at the offending value, not index 0
    let data = r#"{"a": 1, "b": "oops"}"#;
    let err = from_str::<S>(data).unwrap_err();
    assert!(matches!(err, Error::Data { .. }));
    assert_eq!(err.index(), Some(data.find("\"oops\"").unwrap()));
}

#[test]
fn error_resolves_to_line_and_column() {
    #[derive(Deserialize, Debug)]
    #[allow(dead_code)]
    struct S {
        a: i64,
        b: i64,
    }
    // error on line 3 resolves to a line/column
    let data = "{\n  \"a\": 1,\n  \"b\": \"oops\"\n}";
    let err = from_str::<S>(data).unwrap_err();
    let pos = err.get_position(data.as_bytes()).unwrap();
    assert_eq!(pos.line, 3);
    assert!(err.description(data.as_bytes()).contains("line 3 column"));
}

#[test]
fn from_slice_works() {
    let v: Vec<u8> = from_slice(b"[1, 2, 3]").unwrap();
    assert_eq!(v, vec![1, 2, 3]);
}

#[test]
fn recursion_limit_default_rejects_deep_nesting() {
    // beyond the default limit: a clean error, not a stack overflow
    let deep = format!("{}{}", "[".repeat(300), "]".repeat(300));
    let err = from_str::<serde_json::Value>(&deep).unwrap_err();
    assert!(matches!(&err, Error::Syntax(e) if e.error_type == jiter::JsonErrorType::RecursionLimitExceeded));
}

#[test]
fn recursion_limit_configurable() {
    let json = "[[[[[1]]]]]"; // 5 levels deep
    let mut de = JiterDeserializer::new(json.as_bytes()).with_recursion_limit(3);
    assert!(serde_json::Value::deserialize(&mut de).is_err());

    let mut de = JiterDeserializer::new(json.as_bytes()).with_recursion_limit(10);
    assert!(serde_json::Value::deserialize(&mut de).is_ok());
}

#[test]
fn recursion_limit_disabled_allows_deep_nesting() {
    // 300 levels exceeds the default of 200; disabling the limit deserializes it
    let deep = format!("{}1{}", "[".repeat(300), "]".repeat(300));
    let mut de = JiterDeserializer::new(deep.as_bytes()).disable_recursion_limit();
    assert!(serde_json::Value::deserialize(&mut de).is_ok());
    de.finish().unwrap();
}

#[test]
fn extra_tuple_elements_rejected() {
    // too many elements is an error, matching serde_json
    assert_eq!(from_str::<(i64, i64)>("[1, 2]").unwrap(), (1, 2));
    assert!(from_str::<(i64, i64)>("[1, 2, 3]").is_err());
    assert!(from_str::<[i64; 2]>("[1, 2, 3]").is_err());
}

#[derive(Deserialize, Debug, PartialEq)]
enum Tree {
    Leaf,
    Node(Box<Tree>),
}

#[test]
fn recursion_limit_counts_enum_wrappers() {
    let nested = r#"{"Node":{"Node":{"Node":"Leaf"}}}"#; // 3 nested Node objects
    // each enum wrapper counts toward the limit
    let mut de = JiterDeserializer::new(nested.as_bytes()).with_recursion_limit(2);
    assert!(Tree::deserialize(&mut de).is_err());
    // a generous limit accepts it
    let mut de = JiterDeserializer::new(nested.as_bytes()).with_recursion_limit(10);
    assert!(Tree::deserialize(&mut de).is_ok());
}

#[test]
fn unit_variant_object_form() {
    // serde_json accepts `{"Unit": null}` for a unit variant
    assert_eq!(from_str::<Shape>(r#"{"Unit": null}"#).unwrap(), Shape::Unit);
    // the bare-string form still works
    assert_eq!(from_str::<Shape>(r#""Unit""#).unwrap(), Shape::Unit);
    // non-null content for a unit variant is rejected
    assert!(from_str::<Shape>(r#"{"Unit": 5}"#).is_err());
}

#[test]
fn recursion_limit_applies_to_skipped_fields() {
    #[derive(Deserialize, Debug)]
    #[allow(dead_code)]
    struct S {
        keep: i64,
    }
    // a deeply-nested skipped field hits the limit; disabling it accepts it (up to the skip ceiling)
    let deep = format!(r#"{{"skip": {}1{}, "keep": 5}}"#, "[".repeat(230), "]".repeat(230));
    assert!(from_str::<S>(&deep).is_err());

    let mut de = JiterDeserializer::new(deep.as_bytes()).disable_recursion_limit();
    assert_eq!(S::deserialize(&mut de).unwrap().keep, 5);
}

#[test]
fn big_integers() {
    // fits in i128 but not i64
    let n: i128 = from_str("170141183460469231731687303715884105727").unwrap();
    assert_eq!(n, i128::MAX);

    // largest u128
    let n: u128 = from_str("340282366920938463463374607431768211455").unwrap();
    assert_eq!(n, u128::MAX);

    // exceeds i64 -> error when the target is i64
    assert!(from_str::<i64>("99999999999999999999999999999999").is_err());

    // bigger than u128 -> falls back to f64 (lossy), still deserializes as a float
    let huge = format!("1{}", "0".repeat(40)); // 1e40
    let f: f64 = from_str(&huge).unwrap();
    assert!((f - 1e40).abs() / 1e40 < 1e-10);
}

#[test]
fn big_floats() {
    let big: f64 = from_str("1e308").unwrap();
    assert!(big > 1e307 && big.is_finite());

    let small: f64 = from_str("1e-308").unwrap();
    assert!(small > 0.0 && small < 1e-307);

    // a long fractional literal round-trips to the nearest f64
    let pi: f64 = from_str("3.141592653589793238462643383279").unwrap();
    assert!((pi - std::f64::consts::PI).abs() < 1e-15);
}

#[test]
fn nan_and_infinity_rejected_by_default() {
    // not valid JSON unless explicitly allowed (matches serde_json)
    assert!(from_str::<f64>("NaN").is_err());
    assert!(from_str::<f64>("Infinity").is_err());
    assert!(from_str::<f64>("-Infinity").is_err());
}

#[test]
fn nan_and_infinity_allowed_when_enabled() {
    assert!(de_f64_inf_nan("NaN").is_nan());

    let inf = de_f64_inf_nan("Infinity");
    assert!(inf.is_infinite() && inf.is_sign_positive());

    let ninf = de_f64_inf_nan("-Infinity");
    assert!(ninf.is_infinite() && ninf.is_sign_negative());
}

#[test]
fn non_finite_into_value_becomes_string() {
    use serde_json::Value;
    // `serde_json::Value` can't hold non-finite floats; preserved as strings, not dropped to `Null`
    assert_eq!(de_value_inf_nan("Infinity"), Value::String("Infinity".into()));
    assert_eq!(de_value_inf_nan("-Infinity"), Value::String("-Infinity".into()));
    assert_eq!(de_value_inf_nan("NaN"), Value::String("NaN".into()));
    // ordinary floats remain numbers
    assert_eq!(de_value_inf_nan("2.5"), Value::from(2.5));
}

#[test]
fn non_finite_into_f64_keeps_real_value() {
    // typed f64 keeps the real non-finite value
    #[derive(Deserialize)]
    struct S {
        x: f64,
        y: f64,
    }
    let mut de = JiterDeserializer::new(br#"{"x": Infinity, "y": NaN}"#).with_allow_inf_nan();
    let s = S::deserialize(&mut de).unwrap();
    de.finish().unwrap();
    assert!(s.x.is_infinite() && s.x.is_sign_positive());
    assert!(s.y.is_nan());
}

#[test]
fn overflow_into_value_no_longer_null() {
    // 1e400 overflows to inf; into `Value` it becomes a string
    let v: serde_json::Value = from_str("1e400").unwrap();
    assert_eq!(v, serde_json::Value::String("Infinity".into()));
    // into a typed f64 it's the real infinity
    let f: f64 = from_str("1e400").unwrap();
    assert!(f.is_infinite());
}

#[test]
fn matches_serde_json() {
    #[derive(Deserialize, PartialEq, Debug)]
    struct Doc {
        id: u64,
        title: String,
        tags: Vec<String>,
        active: bool,
        score: f64,
        meta: Option<BTreeMap<String, String>>,
    }
    let data = r#"
        {
            "id": 12345,
            "title": "A \"quoted\" title",
            "tags": ["a", "b", "c"],
            "active": true,
            "score": 9.5,
            "meta": {"k": "v"}
        }"#;
    let from_jiter: Doc = from_str(data).unwrap();
    let from_serde: Doc = serde_json::from_str(data).unwrap();
    assert_eq!(from_jiter, from_serde);
}
