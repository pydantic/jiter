use pyo3::prelude::*;
use pyo3::ToPyObject;

use jiter::{python_parse, JsonValue};

#[test]
fn test_to_py_object_numeric() {
    let value =
        JsonValue::parse(br#"  { "int": 1, "bigint": 123456789012345678901234567890, "float": 1.2}  "#).unwrap();
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
    let value = JsonValue::parse(br#"["string", true, false, null]"#).unwrap();
    Python::with_gil(|py| {
        let python_value = value.to_object(py);
        let string = python_value.as_ref(py).to_string();
        assert_eq!(string, "['string', True, False, None]");
    })
}

#[test]
fn test_python_parse_numeric() {
    Python::with_gil(|py| {
        let obj = python_parse(
            py,
            br#"  { "int": 1, "bigint": 123456789012345678901234567890, "float": 1.2}  "#,
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
        let obj = python_parse(py, br#"["string", true, false, null]"#).unwrap();
        assert_eq!(obj.as_ref(py).to_string(), "['string', True, False, None]");
    })
}
