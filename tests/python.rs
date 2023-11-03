use pyo3::prelude::*;
use pyo3::ToPyObject;

use jiter::{python_parse, JsonValue};

#[test]
fn test_to_py_object_numeric() {
    let value = JsonValue::parse(
        br#"  { "int": 1, "bigint": 123456789012345678901234567890, "float": 1.2}  "#,
        false,
    )
    .unwrap();
    Python::with_gil(|py| {
        let python_value = value.to_object(py);
        let string = python_value.as_ref(py).to_string();
        assert_eq!(
            string,
            "{'int': 1, 'bigint': 123456789012345678901234567890, 'float': 1.2}"
        );
    })
}

#[test]
fn test_to_py_object_other() {
    let value = JsonValue::parse(
        br#"["string", "\u00a3", true, false, null, NaN, Infinity, -Infinity]"#,
        true,
    )
    .unwrap();
    Python::with_gil(|py| {
        let python_value = value.to_object(py);
        let string = python_value.as_ref(py).to_string();
        assert_eq!(string, "['string', '£', True, False, None, nan, inf, -inf]");
    })
}

#[test]
fn test_python_parse_numeric() {
    Python::with_gil(|py| {
        let obj = python_parse(
            py,
            br#"  { "int": 1, "bigint": 123456789012345678901234567890, "float": 1.2}  "#,
            false,
        )
        .unwrap();
        assert_eq!(
            obj.as_ref(py).to_string(),
            "{'int': 1, 'bigint': 123456789012345678901234567890, 'float': 1.2}"
        );
    })
}

#[test]
fn test_python_parse_other() {
    Python::with_gil(|py| {
        let obj = python_parse(py, br#"["string", true, false, null, NaN, Infinity, -Infinity]"#, true).unwrap();
        assert_eq!(
            obj.as_ref(py).to_string(),
            "['string', True, False, None, nan, inf, -inf]"
        );
    })
}

#[test]
fn test_python_disallow_nan() {
    Python::with_gil(|py| {
        let e = python_parse(py, br#"[NaN]"#, false).unwrap_err();
        assert_eq!(e.to_string(), "ValueError: expected value at line 1 column 2");
    })
}

#[test]
fn test_error() {
    Python::with_gil(|py| {
        let e = python_parse(py, br#"["string""#, false).unwrap_err();
        assert_eq!(e.to_string(), "ValueError: EOF while parsing a list at line 1 column 9");
    })
}

#[test]
fn test_recursion_limit() {
    let json = (0..10_000).map(|_| "[").collect::<String>();
    let bytes = json.as_bytes();

    Python::with_gil(|py| match python_parse(py, bytes, false) {
        Ok(v) => panic!("unexpectedly valid: {:?}", v),
        Err(e) => {
            assert_eq!(
                e.to_string(),
                "ValueError: recursion limit exceeded at line 1 column 202"
            );
        }
    })
}

#[test]
fn test_recursion_limit_incr() {
    let json = (0..2000).map(|_| "[1]".to_string()).collect::<Vec<_>>().join(", ");
    let json = format!("[{}]", json);
    let bytes = json.as_bytes();

    Python::with_gil(|py| {
        let v = python_parse(py, bytes, false).unwrap();
        assert_eq!(v.as_ref(py).len().unwrap(), 2000);
    })
}
