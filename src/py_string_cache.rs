use std::cell::RefCell;

use ahash::random_state::RandomState;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::sync::{GILOnceCell, GILProtected};
use pyo3::types::{PyBool, PyString};

#[derive(Debug, Clone, Copy)]
pub enum StringCacheMode {
    All,
    Keys,
    None,
}

impl<'py> FromPyObject<'py> for StringCacheMode {
    fn extract_bound(ob: &Bound<'py, PyAny>) -> PyResult<StringCacheMode> {
        if let Ok(bool_mode) = ob.downcast::<PyBool>() {
            Ok(bool_mode.is_true().into())
        } else if let Ok(str_mode) = ob.extract::<&str>() {
            match str_mode {
                "all" => Ok(Self::All),
                "keys" => Ok(Self::Keys),
                "none" => Ok(Self::None),
                _ => Err(PyValueError::new_err(
                    "Invalid string cache mode, should be `'all'`, '`keys`', `'none`' or a `bool`",
                )),
            }
        } else {
            Err(PyTypeError::new_err(
                "Invalid string cache mode, should be `'all'`, '`keys`', `'none`' or a `bool`",
            ))
        }
    }
}

impl From<bool> for StringCacheMode {
    fn from(mode: bool) -> Self {
        if mode {
            Self::All
        } else {
            Self::None
        }
    }
}

pub trait StringMaybeCache {
    fn get_key<'py>(py: Python<'py>, json_str: &str) -> Bound<'py, PyString>;

    fn get_value<'py>(py: Python<'py>, json_str: &str) -> Bound<'py, PyString> {
        Self::get_key(py, json_str)
    }
}

pub struct StringCacheAll;

impl StringMaybeCache for StringCacheAll {
    fn get_key<'py>(py: Python<'py>, json_str: &str) -> Bound<'py, PyString> {
        cached_py_string(py, json_str)
    }
}

pub struct StringCacheKeys;

impl StringMaybeCache for StringCacheKeys {
    fn get_key<'py>(py: Python<'py>, json_str: &str) -> Bound<'py, PyString> {
        cached_py_string(py, json_str)
    }

    fn get_value<'py>(py: Python<'py>, json_str: &str) -> Bound<'py, PyString> {
        PyString::new_bound(py, json_str)
    }
}

pub struct StringNoCache;

impl StringMaybeCache for StringNoCache {
    fn get_key<'py>(py: Python<'py>, json_str: &str) -> Bound<'py, PyString> {
        PyString::new_bound(py, json_str)
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

pub fn cached_py_string<'py>(py: Python<'py>, raw_str: &str) -> Bound<'py, PyString> {
    // from tests, 0 and 1 character strings are faster not cached
    if (2..64).contains(&raw_str.len()) {
        get_string_cache!(py).borrow_mut().get_or_insert(py, raw_str)
    } else {
        PyString::new_bound(py, raw_str)
    }
}

// capacity should be a power of 2 so the compiler can convert `%` to a right shift below
// Using a smaller number here (e.g. 1024) seems to be faster in many cases than a larger number (like 65536)
// and also avoids stack overflow risks
const CAPACITY: usize = 1024;
type Entry = Option<(u64, Py<PyString>)>;

/// This is a Fully associative cache with LRU replacement policy.
/// See https://en.wikipedia.org/wiki/Cache_placement_policies#Fully_associative_cache
#[derive(Debug)]
struct PyStringCache {
    entries: Box<[Entry; CAPACITY]>,
    hash_builder: RandomState,
}

const ARRAY_REPEAT_VALUE: Entry = None;

impl Default for PyStringCache {
    fn default() -> Self {
        Self {
            entries: Box::new([ARRAY_REPEAT_VALUE; CAPACITY]),
            hash_builder: RandomState::default(),
        }
    }
}

impl PyStringCache {
    /// Lookup the cache for an entry with the given string. If it exists, return it.
    /// If it is not set or has a different string, insert it and return it.
    fn get_or_insert<'py>(&mut self, py: Python<'py>, s: &str) -> Bound<'py, PyString> {
        let hash = self.hash_builder.hash_one(s);

        let hash_index = hash as usize % CAPACITY;

        let set_entry = |entry: &mut Entry| {
            let py_str = PyString::new_bound(py, s);
            *entry = Some((hash, py_str.to_owned().unbind()));
            py_str
        };

        // we try up to 5 contiguous slots to find a match or an empty slot
        for index in hash_index..hash_index.wrapping_add(5) {
            if let Some(entry) = self.entries.get_mut(index) {
                if let Some((entry_hash, ref py_str_ob)) = entry {
                    // to avoid a string comparison, we first compare the hashes
                    if *entry_hash == hash {
                        // if the hashes match, we compare the strings to be absolutely sure - as a hashmap would do
                        if py_str_ob.bind(py).to_str().ok() == Some(s) {
                            // the strings matched, return the cached string object
                            return py_str_ob.bind(py).to_owned();
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
        let entry = self.entries.get_mut(hash_index).unwrap();
        set_entry(entry)
    }

    /// get the number of entries in the cache that are set
    fn usage(&self) -> usize {
        self.entries.iter().filter(|e| e.is_some()).count()
    }

    /// clear the cache by resetting all entries to `None`
    fn clear(&mut self) {
        self.entries.fill(None);
    }
}
