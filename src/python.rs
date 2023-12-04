use std::cell::RefCell;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::sync::{GILOnceCell, GILProtected};
use pyo3::types::{PyDict, PyList, PyString};
use pyo3::{ffi, AsPyPointer};

use hashbrown::hash_map::{HashMap, RawEntryMut};
use smallvec::SmallVec;

use crate::errors::{json_err, JsonError, JsonResult, DEFAULT_RECURSION_LIMIT};
use crate::number_decoder::{NumberAny, NumberInt};
use crate::parse::{Parser, Peek};
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

    let peek = python_parser.parser.peek()?;
    let v = if cache_strings {
        python_parser.py_take_value::<StringCache>(py, peek)?
    } else {
        python_parser.py_take_value::<StringNoCache>(py, peek)?
    };
    python_parser.parser.finish()?;
    Ok(v)
}

/// Map a `JsonError` to a `PyErr` which can be raised as an exception in Python as a `ValueError`.
pub fn map_json_error(json_data: &[u8], json_error: &JsonError) -> PyErr {
    PyValueError::new_err(json_error.description(json_data))
}

struct PythonParser<'j> {
    parser: Parser<'j>,
    tape: Tape,
    recursion_limit: u8,
    allow_inf_nan: bool,
}

impl<'j> PythonParser<'j> {
    fn py_take_value<StringCache: StringMaybeCache>(&mut self, py: Python, peek: Peek) -> JsonResult<PyObject> {
        match peek {
            Peek::True => {
                self.parser.consume_true()?;
                Ok(true.to_object(py))
            }
            Peek::False => {
                self.parser.consume_false()?;
                Ok(false.to_object(py))
            }
            Peek::Null => {
                self.parser.consume_null()?;
                Ok(py.None())
            }
            Peek::String => {
                let s = self.parser.consume_string::<StringDecoder>(&mut self.tape)?;
                Ok(StringCache::get(py, s.as_str()))
            }
            Peek::Array => {
                let list = if let Some(peek_first) = self.parser.array_first()? {
                    let mut vec: SmallVec<[PyObject; 8]> = SmallVec::with_capacity(8);
                    let v = self._check_take_value::<StringCache>(py, peek_first)?;
                    vec.push(v);
                    while let Some(peek) = self.parser.array_step()? {
                        let v = self._check_take_value::<StringCache>(py, peek)?;
                        vec.push(v);
                    }
                    PyList::new(py, vec)
                } else {
                    PyList::empty(py)
                };
                Ok(list.to_object(py))
            }
            Peek::Object => {
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
                    let peek = self.parser.peek()?;
                    let first_value = self._check_take_value::<StringCache>(py, peek)?;
                    set_item(first_key, first_value);
                    while let Some(key) = self.parser.object_step::<StringDecoder>(&mut self.tape)? {
                        let key = StringCache::get(py, key.as_str());
                        let peek = self.parser.peek()?;
                        let value = self._check_take_value::<StringCache>(py, peek)?;
                        set_item(key, value);
                    }
                }
                Ok(dict.to_object(py))
            }
            _ => {
                let n = self
                    .parser
                    .consume_number::<NumberAny>(peek.into_inner(), self.allow_inf_nan)?;
                match n {
                    NumberAny::Int(NumberInt::Int(int)) => Ok(int.to_object(py)),
                    NumberAny::Int(NumberInt::BigInt(big_int)) => Ok(big_int.to_object(py)),
                    NumberAny::Float(float) => Ok(float.to_object(py)),
                }
            }
        }
    }

    fn _check_take_value<StringCache: StringMaybeCache>(&mut self, py: Python, peek: Peek) -> JsonResult<PyObject> {
        self.recursion_limit = match self.recursion_limit.checked_sub(1) {
            Some(limit) => limit,
            None => return json_err!(RecursionLimitExceeded, self.parser.index),
        };

        let r = self.py_take_value::<StringCache>(py, peek);

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
        static STRINGS_CACHE: GILOnceCell<GILProtected<RefCell<HashMap<String, PyObject>>>> = GILOnceCell::new();

        // from tests, 0 and 1 character strings are faster not cached
        if (2..64).contains(&json_str.len()) {
            let cache = STRINGS_CACHE
                .get_or_init(py, || GILProtected::new(RefCell::new(HashMap::new())))
                .get(py);

            let mut map = cache.borrow_mut();
            let entry = map.raw_entry_mut().from_key(json_str);

            let (py_string, inserted) = match entry {
                RawEntryMut::Vacant(view) => {
                    let py_string = PyString::new(py, json_str).to_object(py);
                    view.insert(json_str.to_owned(), py_string.clone_ref(py));
                    (py_string, true)
                }
                RawEntryMut::Occupied(view) => (view.get().clone_ref(py), false),
            };
            if inserted {
                // 500k limit means 1m keys + values, 1m 64 byte strings is ~64mb
                if map.len() > 500_000 {
                    // TODO is there a fast way to keep (say) half the cache?
                    map.clear();
                }
            }
            py_string
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
