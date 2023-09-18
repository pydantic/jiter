use pyo3::prelude::*;
use pyo3::ToPyObject;

use jiter::JsonValue;

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
