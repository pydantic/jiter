use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString};
use pyo3::ToPyObject;

use jiter::{cache_clear, cache_usage, map_json_error, python_parse, JsonValue, StringCacheMode};

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
fn test_python_parse_numeric() {
    Python::with_gil(|py| {
        let obj = python_parse(
            py,
            br#"  { "int": 1, "bigint": 123456789012345678901234567890, "float": 1.2}  "#,
            false,
            StringCacheMode::All,
            false,
        )
        .unwrap();
        assert_eq!(
            obj.to_string(),
            "{'int': 1, 'bigint': 123456789012345678901234567890, 'float': 1.2}"
        );
    })
}

#[test]
fn test_python_parse_other_cached() {
    Python::with_gil(|py| {
        let obj = python_parse(
            py,
            br#"["string", true, false, null, NaN, Infinity, -Infinity]"#,
            true,
            StringCacheMode::All,
            false,
        )
        .unwrap();
        assert_eq!(obj.to_string(), "['string', True, False, None, nan, inf, -inf]");
    })
}

#[test]
fn test_python_parse_other_no_cache() {
    Python::with_gil(|py| {
        let obj = python_parse(
            py,
            br#"["string", true, false, null]"#,
            false,
            StringCacheMode::None,
            false,
        )
        .unwrap();
        assert_eq!(obj.to_string(), "['string', True, False, None]");
    })
}

#[test]
fn test_python_disallow_nan() {
    Python::with_gil(|py| {
        let r = python_parse(py, br#"[NaN]"#, false, StringCacheMode::All, false);
        let e = r.map_err(|e| map_json_error(br#"[NaN]"#, &e)).unwrap_err();
        assert_eq!(e.to_string(), "ValueError: expected value at line 1 column 2");
    })
}

#[test]
fn test_error() {
    Python::with_gil(|py| {
        let bytes = br#"["string""#;
        let r = python_parse(py, bytes, false, StringCacheMode::All, false);
        let e = r.map_err(|e| map_json_error(bytes, &e)).unwrap_err();
        assert_eq!(e.to_string(), "ValueError: EOF while parsing a list at line 1 column 9");
    })
}

#[test]
fn test_recursion_limit() {
    let json = (0..10_000).map(|_| "[").collect::<String>();
    let bytes = json.as_bytes();

    Python::with_gil(|py| {
        let r = python_parse(py, bytes, false, StringCacheMode::All, false);
        let e = r.map_err(|e| map_json_error(bytes, &e)).unwrap_err();
        assert_eq!(
            e.to_string(),
            "ValueError: recursion limit exceeded at line 1 column 202"
        );
    })
}

#[test]
fn test_recursion_limit_incr() {
    let json = (0..2000).map(|_| "[1]".to_string()).collect::<Vec<_>>().join(", ");
    let json = format!("[{}]", json);
    let bytes = json.as_bytes();

    Python::with_gil(|py| {
        let v = python_parse(py, bytes, false, StringCacheMode::All, false).unwrap();
        assert_eq!(v.len().unwrap(), 2000);
    });

    Python::with_gil(|py| {
        let v = python_parse(py, bytes, false, StringCacheMode::All, false).unwrap();
        assert_eq!(v.len().unwrap(), 2000);
    });
}

#[test]
fn test_extracted_value_error() {
    let json = "xx";
    let bytes = json.as_bytes();

    Python::with_gil(|py| {
        let r = python_parse(py, bytes, false, StringCacheMode::All, false);
        let e = r.map_err(|e| map_json_error(bytes, &e)).unwrap_err();
        assert_eq!(e.to_string(), "ValueError: expected value at line 1 column 1");
    })
}

#[test]
fn test_partial_array() {
    Python::with_gil(|py| {
        let bytes = br#"["string", true, null, 1, "foo"#;
        let py_value = python_parse(py, bytes, false, StringCacheMode::All, true).unwrap();
        let string = py_value.to_string();
        assert_eq!(string, "['string', True, None, 1]");

        // test that stopping at every points is ok
        for i in 1..bytes.len() {
            let py_value = python_parse(py, &bytes[..i], false, StringCacheMode::All, true).unwrap();
            assert!(py_value.is_instance_of::<PyList>());
        }
    })
}

#[test]
fn test_partial_object() {
    Python::with_gil(|py| {
        let bytes = br#"{"a": 1, "b": 2, "c"#;
        let py_value = python_parse(py, bytes, false, StringCacheMode::All, true).unwrap();
        let string = py_value.to_string();
        assert_eq!(string, "{'a': 1, 'b': 2}");

        // test that stopping at every points is ok
        for i in 1..bytes.len() {
            let py_value = python_parse(py, &bytes[..i], false, StringCacheMode::All, true).unwrap();
            assert!(py_value.is_instance_of::<PyDict>());
        }
    })
}

#[test]
fn test_partial_nested() {
    Python::with_gil(|py| {
        let bytes = br#"{"a": 1, "b": 2, "c": [1, 2, {"d": 1, "#;
        let py_value = python_parse(py, bytes, false, true.into(), true).unwrap();
        let string = py_value.to_string();
        assert_eq!(string, "{'a': 1, 'b': 2, 'c': [1, 2, {'d': 1}]}");

        // test that stopping at every points is ok
        for i in 1..bytes.len() {
            let py_value = python_parse(py, &bytes[..i], false, true.into(), true).unwrap();
            assert!(py_value.is_instance_of::<PyDict>());
        }
    })
}

#[test]
fn test_python_cache_usage_all() {
    Python::with_gil(|py| {
        cache_clear(py);
        let obj = python_parse(py, br#"{"foo": "bar", "spam": 3}"#, true, StringCacheMode::All, false).unwrap();
        assert_eq!(obj.to_string(), "{'foo': 'bar', 'spam': 3}");
        assert_eq!(cache_usage(py), 3);
    })
}

#[test]
fn test_python_cache_usage_keys() {
    Python::with_gil(|py| {
        cache_clear(py);
        let obj = python_parse(py, br#"{"foo": "bar", "spam": 3}"#, false, StringCacheMode::Keys, false).unwrap();
        assert_eq!(obj.to_string(), "{'foo': 'bar', 'spam': 3}");
        assert_eq!(cache_usage(py), 2);
    })
}

#[test]
fn test_python_cache_usage_none() {
    Python::with_gil(|py| {
        cache_clear(py);
        let obj = python_parse(py, br#"{"foo": "bar", "spam": 3}"#, false, StringCacheMode::None, false).unwrap();
        assert_eq!(obj.to_string(), "{'foo': 'bar', 'spam': 3}");
        assert_eq!(cache_usage(py), 0);
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
