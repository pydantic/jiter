use std::cell::{RefCell, RefMut};

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::sync::{GILOnceCell, GILProtected};
use pyo3::types::{PyDict, PyList, PyString};
use pyo3::{ffi, AsPyPointer};

use ahash::AHashMap;
use smallvec::SmallVec;

use crate::errors::{json_error, DEFAULT_RECURSION_LIMIT};
use crate::string_decoder::Tape;
use crate::{FilePosition, JsonError, NumberAny, NumberInt, Parser, Peak, StringDecoder};

pub fn python_parse(py: Python, data: &[u8], cache_strings: bool) -> PyResult<PyObject> {
    let mut python_parser = PythonParser {
        parser: Parser::new(data),
        tape: Tape::default(),
        data,
        recursion_limit: DEFAULT_RECURSION_LIMIT,
    };

    let mje = |e: JsonError| map_json_error(data, e);

    let peak = python_parser.parser.peak().map_err(mje)?;
    let v = if cache_strings {
        python_parser.py_take_value(py, peak, &mut StringCache::new(py))?
    } else {
        python_parser.py_take_value(py, peak, &mut StringNoCache::new(py))?
    };
    python_parser.parser.finish().map_err(mje)?;
    Ok(v)
}

struct PythonParser<'j> {
    parser: Parser<'j>,
    tape: Tape,
    data: &'j [u8],
    recursion_limit: u8,
}

impl<'j> PythonParser<'j> {
    fn py_take_value<'py>(
        &mut self,
        py: Python<'py>,
        peak: Peak,
        strings_cache: &mut impl StringMaybeCache<'py>,
    ) -> PyResult<PyObject> {
        let mje = |e: JsonError| map_json_error(self.data, e);
        match peak {
            Peak::True => {
                self.parser.consume_true().map_err(mje)?;
                Ok(true.to_object(py))
            }
            Peak::False => {
                self.parser.consume_false().map_err(mje)?;
                Ok(false.to_object(py))
            }
            Peak::Null => {
                self.parser.consume_null().map_err(mje)?;
                Ok(py.None())
            }
            Peak::String => {
                let s = self
                    .parser
                    .consume_string::<StringDecoder>(&mut self.tape)
                    .map_err(mje)?;
                Ok(strings_cache.get(py, s.as_str()))
            }
            Peak::Num(first) => {
                let n = self.parser.consume_number::<NumberAny>(first).map_err(mje)?;
                match n {
                    NumberAny::Int(NumberInt::Int(int)) => Ok(int.to_object(py)),
                    NumberAny::Int(NumberInt::BigInt(big_int)) => Ok(big_int.to_object(py)),
                    NumberAny::Float(float) => Ok(float.to_object(py)),
                }
            }
            Peak::Array => {
                let list = if let Some(peak_first) = self.parser.array_first().map_err(mje)? {
                    let mut vec: SmallVec<[PyObject; 8]> = SmallVec::with_capacity(8);
                    let v = self._check_take_value(py, peak_first, strings_cache)?;
                    vec.push(v);
                    while let Some(peak) = self.parser.array_step().map_err(mje)? {
                        let v = self._check_take_value(py, peak, strings_cache)?;
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

                if let Some(first_key) = self.parser.object_first::<StringDecoder>(&mut self.tape).map_err(mje)? {
                    let first_key = strings_cache.get(py, first_key.as_str());
                    let peak = self.parser.peak().map_err(mje)?;
                    let first_value = self._check_take_value(py, peak, strings_cache)?;
                    set_item(first_key, first_value);
                    while let Some(key) = self.parser.object_step::<StringDecoder>(&mut self.tape).map_err(mje)? {
                        let key = strings_cache.get(py, key.as_str());
                        let peak = self.parser.peak().map_err(mje)?;
                        let value = self._check_take_value(py, peak, strings_cache)?;
                        set_item(key, value);
                    }
                }
                Ok(dict.to_object(py))
            }
        }
    }

    fn _check_take_value<'py>(
        &mut self,
        py: Python<'py>,
        peak: Peak,
        strings_cache: &mut impl StringMaybeCache<'py>,
    ) -> PyResult<PyObject> {
        self.recursion_limit = match self.recursion_limit.checked_sub(1) {
            Some(limit) => limit,
            None => {
                return Err(map_json_error(
                    self.data,
                    json_error!(RecursionLimitExceeded, self.parser.index),
                ))
            }
        };

        let r = self.py_take_value(py, peak, strings_cache);

        self.recursion_limit += 1;
        r
    }
}

fn map_json_error(data: &[u8], json_error: JsonError) -> PyErr {
    let JsonError { error_type, index } = json_error;
    let position = FilePosition::find(data, index);
    let msg = format!("{} at {}", error_type, position);
    PyValueError::new_err(msg)
}

trait StringMaybeCache<'py> {
    fn new(py: Python<'py>) -> Self;

    fn get(&mut self, py: Python, json_str: &str) -> PyObject;
}

struct StringCache<'py>;

impl<'py> StringMaybeCache<'py> for StringCache<'py> {
    fn new(py: Python<'py>) -> Self {
        Self
    }

    fn get(&mut self, py: Python, json_str: &str) -> PyObject {
        static STRINGS_CACHE: GILOnceCell<GILProtected<RefCell<AHashMap<String, PyObject>>>> = GILOnceCell::new();

        if json_str.len() < 64 {
            let cache = STRINGS_CACHE
                .get_or_init(py, || GILProtected::new(RefCell::new(AHashMap::new())))
                .get();
            // Finish the borrow before matching, so that the RefCell isn't borrowed for the
            // whole match.
            let key = cache.borrow().get(json_str).map(|key| key.clone_ref(py));

            match key {
                Some(key) => key,
                None => {
                    let key_object = PyString::new(py, json_str).to_object(py);
                    let cache_writable = cache.borrow_mut();
                    if cache_writable.len() > 100_000 {
                        cache_writable.clear();
                    }
                    cache_writable.insert(json_str.to_string(), key_object.clone_ref(py));
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

impl StringMaybeCache<'_> for StringNoCache {
    fn new(_py: Python) -> Self {
        Self
    }

    fn get(&mut self, py: Python, json_str: &str) -> PyObject {
        PyString::new(py, json_str).to_object(py)
    }
}
