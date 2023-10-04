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

static KEYS_CACHE: GILOnceCell<GILProtected<RefCell<AHashMap<String, PyObject>>>> = GILOnceCell::new();

pub fn python_parse(py: Python, data: &[u8]) -> PyResult<PyObject> {
    let protected_cache = KEYS_CACHE.get_or_init(py, || GILProtected::new(RefCell::new(AHashMap::new())));
    let mut cache: RefMut<AHashMap<String, PyObject>> = protected_cache.get(py).borrow_mut();

    if cache.len() > 100_000 {
        cache.clear();
    }

    let mut python_parser = PythonParser {
        parser: Parser::new(data),
        tape: Tape::default(),
        data,
        recursion_limit: DEFAULT_RECURSION_LIMIT,
    };

    let mje = |e: JsonError| map_json_error(data, e);

    let peak = python_parser.parser.peak().map_err(mje)?;
    let v = python_parser.py_take_value(py, peak, &mut *cache)?;
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
    fn py_take_value(
        &mut self,
        py: Python,
        peak: Peak,
        keys_hashmap: &mut AHashMap<String, PyObject>,
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
                Ok(PyString::new(py, s.as_str()).to_object(py))
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
                    let v = self._check_take_value(py, peak_first, keys_hashmap)?;
                    vec.push(v);
                    while let Some(peak) = self.parser.array_step().map_err(mje)? {
                        let v = self._check_take_value(py, peak, keys_hashmap)?;
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

                macro_rules! cache_key {
                    ($key:ident) => {
                        if $key.len() < 64 {
                            match keys_hashmap.get($key) {
                                Some(key) => key.clone_ref(py),
                                None => {
                                    let key_object = PyString::new(py, $key).to_object(py);
                                    // shame we have to cache the key again here, is there way to use `entry()`
                                    // without calling `.to_string()` in the case where the key is already cached?
                                    keys_hashmap.insert($key.to_string(), key_object.clone_ref(py));
                                    key_object
                                }
                            }
                        } else {
                            let key = PyString::new(py, $key);
                            key.to_object(py)
                        }
                    };
                }

                if let Some(first_key) = self.parser.object_first::<StringDecoder>(&mut self.tape).map_err(mje)? {
                    let first_key_str = first_key.as_str();
                    let first_key = key_hash!(first_key_str);
                    let peak = self.parser.peak().map_err(mje)?;
                    let first_value = self._check_take_value(py, peak, keys_hashmap)?;
                    set_item(first_key, first_value);
                    while let Some(key) = self.parser.object_step::<StringDecoder>(&mut self.tape).map_err(mje)? {
                        let key_str = key.as_str();
                        let key = key_hash!(key_str);
                        let peak = self.parser.peak().map_err(mje)?;
                        let value = self._check_take_value(py, peak, keys_hashmap)?;
                        set_item(key, value);
                    }
                }
                Ok(dict.to_object(py))
            }
        }
    }

    fn _check_take_value(
        &mut self,
        py: Python,
        peak: Peak,
        keys_hashmap: &mut AHashMap<String, PyObject>,
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

        let r = self.py_take_value(py, peak, keys_hashmap);

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
