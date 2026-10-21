use pyo3::prelude::*;
use pyo3::types::PyString;

use jiter::{JsonValue, PythonParse, StringCacheMode, pystring_ascii_new};

/// The string cache is process-global, so tests that parse with it on, or that read `cache_usage`,
/// take this first: the harness runs the tests in this binary on several threads at once.
static CACHE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn cache_lock() -> std::sync::MutexGuard<'static, ()> {
    CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

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
        // SAFETY: `json` contains only ASCII characters.
        let s = unsafe { pystring_ascii_new(py, json) };
        assert_eq!(s.to_string(), "100abc");
    });
}

#[test]
fn test_python_parse_default() {
    let _cache = cache_lock();
    Python::attach(|py| {
        let v = PythonParse::default().python_parse(py, b"[123]").unwrap();
        assert_eq!(v.to_string(), "[123]");
    });
}

#[test]
fn test_string_cache_pool() {
    // one test, not several: these assertions read the process-global pool, so they must not run
    // beside each other
    let _cache = cache_lock();
    let json = br#"{"some_key": "some_value", "another_key": "another_value"}"#;
    Python::attach(|py| {
        // dropping cached strings releases Python objects, so clear while attached
        jiter::cache_clear();
        let parse = PythonParse {
            cache_mode: StringCacheMode::All,
            ..Default::default()
        };
        parse.python_parse(py, json).unwrap();
        // the parse put its cache back, with the keys and values it interned still in it
        assert_eq!(jiter::cache_usage(), 4);
        // a second parse takes that same cache back out and finds them already there
        parse.python_parse(py, json).unwrap();
        assert_eq!(jiter::cache_usage(), 4);
        jiter::cache_clear();
        assert_eq!(jiter::cache_usage(), 0);
    });

    // parses from several threads must not lose or double-free a cache, and must not deadlock
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                Python::attach(|py| {
                    let parse = PythonParse {
                        cache_mode: StringCacheMode::All,
                        ..Default::default()
                    };
                    for _ in 0..200 {
                        parse.python_parse(py, json).unwrap();
                    }
                });
            });
        }
    });
    // every cache a thread built came back to the pool or was dropped, and each holds the same
    // four strings, so usage is a multiple of four and bounded by the pool
    Python::attach(|_| {
        let usage = jiter::cache_usage();
        assert_eq!(usage % 4, 0, "unexpected cache usage {usage}");
        assert!(usage <= 4 * 8, "pool grew past its bound: {usage}");
        jiter::cache_clear();
    });
}
