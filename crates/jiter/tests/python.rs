use pyo3::prelude::*;
use pyo3::types::PyString;

use jiter::{pystring_ascii_new, JsonValue, PythonParse, StringCacheMode};

#[cfg(feature = "num-bigint")]
#[test]
fn test_to_py_object_numeric() {
    let value = JsonValue::parse(
        br#"  { "int": 1, "bigint": 123456789012345678901234567890, "float": 1.2}  "#,
        false,
    )
    .unwrap();
    Python::attach(|py| {
        let python_value = value.into_pyobject(py).unwrap();
        let string = python_value.to_string();
        assert_eq!(
            string,
            "{'int': 1, 'bigint': 123456789012345678901234567890, 'float': 1.2}"
        );
    });
}

#[test]
fn test_to_py_object_other() {
    let value = JsonValue::parse(
        br#"["string", "\u00a3", true, false, null, NaN, Infinity, -Infinity]"#,
        true,
    )
    .unwrap();
    Python::attach(|py| {
        let python_value = value.into_pyobject(py).unwrap();
        let string = python_value.to_string();
        assert_eq!(string, "['string', '£', True, False, None, nan, inf, -inf]");
    });
}

#[test]
fn test_cache_into() {
    Python::attach(|py| {
        let c: StringCacheMode = true.into_pyobject(py).unwrap().extract().unwrap();
        assert!(matches!(c, StringCacheMode::All));

        let c: StringCacheMode = false.into_pyobject(py).unwrap().extract().unwrap();
        assert!(matches!(c, StringCacheMode::None));

        let c: StringCacheMode = PyString::new(py, "all").extract().unwrap();
        assert!(matches!(c, StringCacheMode::All));

        let c: StringCacheMode = PyString::new(py, "keys").extract().unwrap();
        assert!(matches!(c, StringCacheMode::Keys));

        let c: StringCacheMode = PyString::new(py, "none").extract().unwrap();
        assert!(matches!(c, StringCacheMode::None));

        let e = PyString::new(py, "wrong").extract::<StringCacheMode>().unwrap_err();
        assert_eq!(
            e.to_string(),
            "ValueError: Invalid string cache mode, should be `'all'`, '`keys`', `'none`' or a `bool`"
        );
        let e = 123i32
            .into_pyobject(py)
            .unwrap()
            .extract::<StringCacheMode>()
            .unwrap_err();
        assert_eq!(
            e.to_string(),
            "TypeError: Invalid string cache mode, should be `'all'`, '`keys`', `'none`' or a `bool`"
        );
    });
}

#[test]
fn test_pystring_ascii_new() {
    let json = "100abc";
    Python::attach(|py| {
        let s = unsafe { pystring_ascii_new(py, json) };
        assert_eq!(s.to_string(), "100abc");
    });
}

#[test]
fn test_python_parse_default() {
    Python::attach(|py| {
        let v = PythonParse::default().python_parse(py, b"[123]").unwrap();
        assert_eq!(v.to_string(), "[123]");
    });
}
