use std::cell::RefCell;
use std::hash::{BuildHasher, BuildHasherDefault};

use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::sync::{GILOnceCell, GILProtected};
use pyo3::types::{PyBool, PyString};

use ahash::AHasher;

#[derive(Debug, Clone, Copy)]
pub enum StringCacheMode {
    All,
    Keys,
    None,
}

impl TryFrom<&PyAny> for StringCacheMode {
    type Error = PyErr;

    fn try_from(mode: &PyAny) -> PyResult<Self> {
        if let Ok(bool_mode) = mode.downcast::<PyBool>() {
            Ok(if bool_mode.is_true() { Self::All } else { Self::None })
        } else {
            match mode.extract()? {
                "all" => Ok(Self::All),
                "keys" => Ok(Self::Keys),
                "none" => Ok(Self::None),
                _ => Err(PyTypeError::new_err(format!("Invalid string cache mode: {}", mode))),
            }
        }
    }
}

pub trait StringMaybeCache {
    fn get_key(py: Python, json_str: &str) -> PyObject;

    fn get_value(py: Python, json_str: &str) -> PyObject {
        Self::get_key(py, json_str)
    }
}

pub struct StringCacheAll;

impl StringMaybeCache for StringCacheAll {
    fn get_key(py: Python, json_str: &str) -> PyObject {
        cache_get_or_insert(py, json_str)
    }
}

pub struct StringCacheKeys;

impl StringMaybeCache for StringCacheKeys {
    fn get_key(py: Python, json_str: &str) -> PyObject {
        cache_get_or_insert(py, json_str)
    }

    fn get_value(py: Python, json_str: &str) -> PyObject {
        PyString::new(py, json_str).to_object(py)
    }
}

pub struct StringNoCache;

impl StringMaybeCache for StringNoCache {
    fn get_key(py: Python, json_str: &str) -> PyObject {
        PyString::new(py, json_str).to_object(py)
    }
}

static STRING_CACHE: GILOnceCell<GILProtected<RefCell<PyStringCache>>> = GILOnceCell::new();

macro_rules! get_string_cache {
    ($py:ident) => {
        STRING_CACHE
            .get_or_init($py, || GILProtected::new(RefCell::new(PyStringCache::default())))
            .get($py)
    };
}

pub fn cache_usage(py: Python) -> usize {
    get_string_cache!(py).borrow().usage()
}

pub fn cache_clear(py: Python) {
    get_string_cache!(py).borrow_mut().clear()
}

fn cache_get_or_insert(py: Python, json_str: &str) -> PyObject {
    // from tests, 0 and 1 character strings are faster not cached
    if (2..64).contains(&json_str.len()) {
        get_string_cache!(py).borrow_mut().get_or_insert(py, json_str)
    } else {
        PyString::new(py, json_str).to_object(py)
    }
}

// capacity should be a power of 2 so the compiler can convert `%` to a right shift below
const CAPACITY: usize = 65536;

#[derive(Debug)]
struct PyStringCache {
    entries: Vec<Option<(u64, Py<PyString>)>>,
    hash_builder: BuildHasherDefault<AHasher>,
}

impl Default for PyStringCache {
    fn default() -> Self {
        Self {
            entries: vec![None; CAPACITY],
            hash_builder: BuildHasherDefault::default(),
        }
    }
}

impl PyStringCache {
    /// Lookup the cache for an entry with the given string. If it exists, return it.
    /// If it is not set or has a different string, insert it and return it.
    fn get_or_insert(&mut self, py: Python, s: &str) -> PyObject {
        let hash = self.hash_builder.hash_one(s);

        let hash_index = hash as usize % CAPACITY;

        let set_entry = |entry: &mut Option<(u64, Py<PyString>)>| {
            let py_str = PyString::new(py, s);
            *entry = Some((hash, py_str.into_py(py)));
            py_str.to_object(py)
        };

        for index in hash_index..(hash_index + 5) {
            if let Some(entry) = self.entries.get_mut(index) {
                if let Some((entry_hash, ref py_str_ob)) = entry {
                    // to avoid a string comparison, we first compare the hashes
                    if *entry_hash == hash {
                        // if the hashes match, we compare the strings to be absolutely sure - as a hashmap would do
                        if py_str_ob.as_ref(py).to_str().ok() == Some(s) {
                            return py_str_ob.to_object(py);
                        }
                    }
                } else {
                    // we got to an empty entry, use it
                    return set_entry(entry);
                }
            } else {
                // we reached the end of entries, break
                break;
            }
        }
        // we tried all 5 slots (or got to the end of entries) without finding a match
        // or an empty slot, make this LRU by replacing the first entry
        let entry = unsafe { self.entries.get_unchecked_mut(hash_index) };
        set_entry(entry)
    }

    /// get the number of entries in the cache that are set
    fn usage(&self) -> usize {
        self.entries.iter().filter(|e| e.is_some()).count()
    }

    /// clear the cache by resetting all entries to `None`
    fn clear(&mut self) {
        self.entries.clear();
        self.entries.resize(CAPACITY, None);
    }
}
