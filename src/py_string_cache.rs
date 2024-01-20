use std::cell::RefCell;
use std::hash::{BuildHasher, BuildHasherDefault};

use pyo3::prelude::*;
use pyo3::sync::{GILOnceCell, GILProtected};
use pyo3::types::PyString;

use ahash::AHasher;

pub trait StringMaybeCache {
    fn get_key(py: Python, json_str: &str) -> PyObject;

    fn get_value(py: Python, json_str: &str) -> PyObject {
        Self::get_key(py, json_str)
    }
}

static STRINGS_CACHE: GILOnceCell<GILProtected<RefCell<PyStringCache>>> = GILOnceCell::new();

pub fn cache_usage(py: Python) -> usize {
    STRINGS_CACHE
        .get_or_init(py, || GILProtected::new(RefCell::new(PyStringCache::new())))
        .get(py)
        .borrow()
        .usage()
}

pub fn cache_clear(py: Python) {
    STRINGS_CACHE
        .get_or_init(py, || GILProtected::new(RefCell::new(PyStringCache::new())))
        .get(py)
        .borrow_mut()
        .clear()
}

fn cache_get(py: Python, json_str: &str) -> PyObject {
    // from tests, 0 and 1 character strings are faster not cached
    if (2..64).contains(&json_str.len()) {
        let cache_ref_cell = STRINGS_CACHE
            .get_or_init(py, || GILProtected::new(RefCell::new(PyStringCache::new())))
            .get(py);

        cache_ref_cell.borrow_mut().get_or_insert(py, json_str)
    } else {
        PyString::new(py, json_str).to_object(py)
    }
}

pub struct StringCacheAll;

impl StringMaybeCache for StringCacheAll {
    fn get_key(py: Python, json_str: &str) -> PyObject {
        cache_get(py, json_str)
    }
}

pub struct StringCacheKeys;

impl StringMaybeCache for StringCacheKeys {
    fn get_key(py: Python, json_str: &str) -> PyObject {
        cache_get(py, json_str)
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

// capacity should be a power of 2 so the compiler can convert `%` to a right shift below
const CAPACITY: usize = 65536;

#[derive(Debug)]
struct PyStringCache {
    entries: Vec<Option<(u64, Py<PyString>)>>,
    hash_builder: BuildHasherDefault<AHasher>,
}

impl PyStringCache {
    fn new() -> Self {
        Self {
            entries: vec![None; CAPACITY],
            hash_builder: BuildHasherDefault::default(),
        }
    }

    fn get_or_insert(&mut self, py: Python, s: &str) -> PyObject {
        let hash = self.hash_builder.hash_one(s);

        let index = hash as usize % CAPACITY;

        let entry = unsafe { self.entries.get_unchecked_mut(index) };
        if let Some((h, ref py_str_ob)) = entry {
            // to avoid a string comparison, we first compare the hashes
            if *h == hash {
                // if the hashes match, we compare the strings to be correct - as hashmap would do
                if py_str_ob.as_ref(py).to_str().ok() == Some(s) {
                    return py_str_ob.to_object(py);
                }
            }
        }

        let py_str = PyString::new(py, s);
        *entry = Some((hash, py_str.into_py(py)));
        py_str.to_object(py)
    }

    // get usage proportion and clear the cache
    fn usage(&self) -> usize {
        self.entries.iter().filter(|e| e.is_some()).count()
    }
    fn clear(&mut self) {
        self.entries.clear();
        self.entries.resize(CAPACITY, None);
    }
}
