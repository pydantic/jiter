use std::cell::RefCell;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::sync::{GILOnceCell, GILProtected};
use pyo3::types::{PyDict, PyList, PyString};
use pyo3::{ffi, AsPyPointer};

use ahash::AHashMap;
use smallvec::SmallVec;

use crate::errors::{json_err, FilePosition, JsonError, JsonResult, DEFAULT_RECURSION_LIMIT};
use crate::number_decoder::{NumberAny, NumberInt};
use crate::parse::{Parser, Peak};
use crate::string_decoder::{StringDecoder, Tape};

/// Parse a JSON value from a byte slice and return a Python object.
///
/// # Arguments
/// - `py`: [Python](https://docs.rs/pyo3/latest/pyo3/marker/struct.Python.html) marker token.
/// - `json_data`: The JSON data to parse.
/// - `allow_inf_nan`: Whether to allow `(-)Infinity` and `NaN` values.
/// - `cache_strings`: Whether to cache strings to avoid constructing new Python objects,
/// this should have a significant improvement on performance but increases memory slightly.
///
/// # Returns
///
/// A [PyObject](https://docs.rs/pyo3/latest/pyo3/type.PyObject.html) representing the parsed JSON value.
pub fn python_parse(py: Python, json_data: &[u8], allow_inf_nan: bool, cache_strings: bool) -> JsonResult<PyObject> {
    let mut python_parser = PythonParser {
        parser: Parser::new(json_data),
        tape: Tape::default(),
        recursion_limit: DEFAULT_RECURSION_LIMIT,
        allow_inf_nan,
    };

    let peak = python_parser.parser.peak()?;
    let v = if cache_strings {
        python_parser.py_take_value::<StringCache>(py, peak)?
    } else {
        python_parser.py_take_value::<StringNoCache>(py, peak)?
    };
    python_parser.parser.finish()?;
    Ok(v)
}

/// Map a `JsonError` to a `PyErr` which can be raised as an exception in Python as a `ValueError`.
pub fn map_json_error(json_data: &[u8], json_error: JsonError) -> PyErr {
    let JsonError { error_type, index } = json_error;
    let position = FilePosition::find(json_data, index);
    let msg = format!("{} at {}", error_type, position);
    PyValueError::new_err(msg)
}

struct PythonParser<'j> {
    parser: Parser<'j>,
    tape: Tape,
    recursion_limit: u8,
    allow_inf_nan: bool,
}

impl<'j> PythonParser<'j> {
    fn py_take_value<StringCache: StringMaybeCache>(&mut self, py: Python, peak: Peak) -> JsonResult<PyObject> {
        match peak {
            Peak::True => {
                self.parser.consume_true()?;
                Ok(true.to_object(py))
            }
            Peak::False => {
                self.parser.consume_false()?;
                Ok(false.to_object(py))
            }
            Peak::Null => {
                self.parser.consume_null()?;
                Ok(py.None())
            }
            Peak::String => {
                let s = self.parser.consume_string::<StringDecoder>(&mut self.tape)?;
                Ok(StringCache::get(py, s.as_str()))
            }
            Peak::Num(first) => {
                let n = self.parser.consume_number::<NumberAny>(first, self.allow_inf_nan)?;
                match n {
                    NumberAny::Int(NumberInt::Int(int)) => Ok(int.to_object(py)),
                    NumberAny::Int(NumberInt::BigInt(big_int)) => Ok(big_int.to_object(py)),
                    NumberAny::Float(float) => Ok(float.to_object(py)),
                }
            }
            Peak::Array => {
                let list = if let Some(peak_first) = self.parser.array_first()? {
                    let mut vec: SmallVec<[PyObject; 8]> = SmallVec::with_capacity(8);
                    let v = self._check_take_value::<StringCache>(py, peak_first)?;
                    vec.push(v);
                    while let Some(peak) = self.parser.array_step()? {
                        let v = self._check_take_value::<StringCache>(py, peak)?;
                        vec.push(v);
                    }
                    PyList::new(py, vec)
                } else {
                    PyList::empty(py)
                };
                Ok(list.to_object(py))
            }
            Peak::Object => {
                let dict = PyDict::new(py);

                let set_item = |key: PyObject, value: PyObject| {
                    let r = unsafe { ffi::PyDict_SetItem(dict.as_ptr(), key.as_ptr(), value.as_ptr()) };
                    // AFAIK this shouldn't happen since the key will always be a string  which is hashable
                    // we panic here rather than returning a result and using `?` below as it's up to 14% faster
                    // presumably because there are fewer branches
                    if r == -1 {
                        panic!("PyDict_SetItem failed")
                    }
                };

                if let Some(first_key) = self.parser.object_first::<StringDecoder>(&mut self.tape)? {
                    let first_key = StringCache::get(py, first_key.as_str());
                    let peak = self.parser.peak()?;
                    let first_value = self._check_take_value::<StringCache>(py, peak)?;
                    set_item(first_key, first_value);
                    while let Some(key) = self.parser.object_step::<StringDecoder>(&mut self.tape)? {
                        let key = StringCache::get(py, key.as_str());
                        let peak = self.parser.peak()?;
                        let value = self._check_take_value::<StringCache>(py, peak)?;
                        set_item(key, value);
                    }
                }
                Ok(dict.to_object(py))
            }
        }
    }

    fn _check_take_value<StringCache: StringMaybeCache>(&mut self, py: Python, peak: Peak) -> JsonResult<PyObject> {
        self.recursion_limit = match self.recursion_limit.checked_sub(1) {
            Some(limit) => limit,
            None => return json_err!(RecursionLimitExceeded, self.parser.index),
        };

        let r = self.py_take_value::<StringCache>(py, peak);

        self.recursion_limit += 1;
        r
    }
}

trait StringMaybeCache {
    fn get(py: Python, json_str: &str) -> PyObject;
}

struct StringCache;

impl StringMaybeCache for StringCache {
    fn get(py: Python, json_str: &str) -> PyObject {
        static STRINGS_CACHE: GILOnceCell<GILProtected<RefCell<AHashMap<String, PyObject>>>> = GILOnceCell::new();

        if json_str.len() < 64 {
            let cache = STRINGS_CACHE
                .get_or_init(py, || GILProtected::new(RefCell::new(AHashMap::new())))
                .get(py);

            // Finish the borrow before matching, so that the RefCell isn't borrowed for the whole match.
            let key = cache.borrow().get(json_str).map(|key| key.clone_ref(py));

            match key {
                Some(key) => key,
                None => {
                    let key_object = PyString::new(py, json_str).to_object(py);
                    let mut cache_writable = cache.borrow_mut();
                    if cache_writable.len() > 100_000 {
                        cache_writable.clear();
                    }
                    cache_writable.insert(json_str.to_owned(), key_object.clone_ref(py));
                    key_object
                }
            }
        } else {
            let key = PyString::new(py, json_str);
            key.to_object(py)
        }
    }
}

struct StringNoCache;

impl StringMaybeCache for StringNoCache {
    fn get(py: Python, json_str: &str) -> PyObject {
        PyString::new(py, json_str).to_object(py)
    }
}
