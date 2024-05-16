use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString};

use jiter::{cache_clear, cache_usage, map_json_error, pystring_fast_new, python_parse, JsonValue, StringCacheMode};

#[test]
fn test_to_py_object_numeric() {
    let value = JsonValue::parse(
        br#"  { "int": 1, "bigint": 123456789012345678901234567890, "float": 1.2}  "#,
        false,
    )
    .unwrap();
    Python::with_gil(|py| {
        let python_value = value.to_object(py);
        let string = python_value.bind(py).to_string();
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
        let string = python_value.bind(py).to_string();
        assert_eq!(string, "['string', '£', True, False, None, nan, inf, -inf]");
    })
}

#[test]
fn test_cache_into() {
    Python::with_gil(|py| {
        let c: StringCacheMode = true.to_object(py).extract(py).unwrap();
        assert!(matches!(c, StringCacheMode::All));

        let c: StringCacheMode = false.to_object(py).extract(py).unwrap();
        assert!(matches!(c, StringCacheMode::None));

        let c: StringCacheMode = PyString::new_bound(py, "all").extract().unwrap();
        assert!(matches!(c, StringCacheMode::All));

        let c: StringCacheMode = PyString::new_bound(py, "keys").extract().unwrap();
        assert!(matches!(c, StringCacheMode::Keys));

        let c: StringCacheMode = PyString::new_bound(py, "none").extract().unwrap();
        assert!(matches!(c, StringCacheMode::None));

        let e = PyString::new_bound(py, "wrong")
            .extract::<StringCacheMode>()
            .unwrap_err();
        assert_eq!(
            e.to_string(),
            "ValueError: Invalid string cache mode, should be `'all'`, '`keys`', `'none`' or a `bool`"
        );
        let e = 123.to_object(py).extract::<StringCacheMode>(py).unwrap_err();
        assert_eq!(
            e.to_string(),
            "TypeError: Invalid string cache mode, should be `'all'`, '`keys`', `'none`' or a `bool`"
        );
    })
}

#[test]
fn test_pystring_fast_new_non_ascii() {
    let json = "£100 💩";
    Python::with_gil(|py| {
        let s = pystring_fast_new(py, json, false);
        assert_eq!(s.to_string(), "£100 💩");
    })
}

#[test]
fn test_pystring_fast_new_ascii() {
    let json = "100abc";
    Python::with_gil(|py| {
        let s = pystring_fast_new(py, json, true);
        assert_eq!(s.to_string(), "100abc");
    })
}
