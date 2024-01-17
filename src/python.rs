use std::cell::RefCell;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::sync::{GILOnceCell, GILProtected};
use pyo3::types::{PyDict, PyList, PyString};
use pyo3::{ffi, FromPyPointer};

use hashbrown::hash_map::{HashMap, RawEntryMut};
use pyo3::ffi::{PyASCIIObject, PyCompactUnicodeObject};
use smallvec::SmallVec;

use crate::errors::{json_err, json_error, JsonError, JsonResult, DEFAULT_RECURSION_LIMIT};
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
pub fn python_parse<'py>(
    py: Python<'py>,
    json_data: &[u8],
    allow_inf_nan: bool,
    cache_strings: bool,
) -> JsonResult<Bound<'py, PyAny>> {
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
    fn py_take_value<'py, StringCache: StringMaybeCache>(
        &mut self,
        py: Python<'py>,
        peek: Peek,
    ) -> JsonResult<Bound<'py, PyAny>> {
        match peek {
            Peek::Null => {
                self.parser.consume_null()?;
                Ok(py.None().to_object(py).into_bound(py))
            }
            Peek::True => {
                self.parser.consume_true()?;
                Ok(true.to_object(py).into_bound(py))
            }
            Peek::False => {
                self.parser.consume_false()?;
                Ok(false.to_object(py).into_bound(py))
            }
            Peek::String => {
                let s = self.parser.consume_string::<StringDecoder>(&mut self.tape)?;
                Ok(StringCache::get(py, s.as_str()))
            }
            Peek::Array => {
                let list = if let Some(peek_first) = self.parser.array_first()? {
                    let mut vec: SmallVec<[Bound<'_, PyAny>; 8]> = SmallVec::with_capacity(8);
                    let v = self._check_take_value::<StringCache>(py, peek_first)?;
                    vec.push(v);
                    while let Some(peek) = self.parser.array_step()? {
                        let v = self._check_take_value::<StringCache>(py, peek)?;
                        vec.push(v);
                    }
                    PyList::new_bound(py, vec)
                } else {
                    PyList::empty_bound(py)
                };
                Ok(list.to_object(py).into_bound(py))
            }
            Peek::Object => {
                let dict = PyDict::new_bound(py);

                let set_item = |key: Bound<'py, PyAny>, value: Bound<'py, PyAny>| {
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
                Ok(dict.to_object(py).into_bound(py))
            }
            _ => {
                let n = self
                    .parser
                    .consume_number::<NumberAny>(peek.into_inner(), self.allow_inf_nan);
                match n {
                    Ok(NumberAny::Int(NumberInt::Int(int))) => Ok(int.to_object(py).into_bound(py)),
                    Ok(NumberAny::Int(NumberInt::BigInt(big_int))) => Ok(big_int.to_object(py).into_bound(py)),
                    Ok(NumberAny::Float(float)) => Ok(float.to_object(py).into_bound(py)),
                    Err(e) => {
                        if !peek.is_num() {
                            Err(json_error!(ExpectedSomeValue, self.parser.index))
                        } else {
                            Err(e)
                        }
                    }
                }
            }
        }
    }

    fn _check_take_value<'py, StringCache: StringMaybeCache>(
        &mut self,
        py: Python<'py>,
        peek: Peek,
    ) -> JsonResult<Bound<'py, PyAny>> {
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
    fn get<'py>(py: Python<'py>, json_str: &str) -> Bound<'py, PyAny>;
}

struct StringCache;

impl StringMaybeCache for StringCache {
    fn get<'py>(py: Python<'py>, json_str: &str) -> Bound<'py, PyAny> {
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
                    let py_string = py_str_from_str(py, json_str);
                    view.insert(json_str.to_owned(), py_string.clone().into());
                    (py_string, true)
                }
                RawEntryMut::Occupied(view) => (view.get().bind(py).clone(), false),
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
            py_str_from_str(py, json_str)
        }
    }
}

struct StringNoCache;

impl StringMaybeCache for StringNoCache {
    fn get<'py>(py: Python<'py>, json_str: &str) -> Bound<'py, PyAny> {
        py_str_from_str(py, json_str)
    }
}

enum PyUnicodeKind {
    Ascii,
    OneByte,
    TwoByte,
    FourByte,
}
static EMPTY_STRING: GILOnceCell<PyObject> = GILOnceCell::new();

fn py_str_from_str<'py>(py: Python<'py>, buf: &str) -> Bound<'py, PyAny> {
    if buf.is_empty() {
        return EMPTY_STRING
            .get_or_init(py, || PyString::intern_bound(py, "").to_object(py))
            .to_object(py)
            .into_bound(py);
    } else {
        let ob = unicode_from_str(buf);
        let py_any = unsafe { PyAny::from_owned_ptr(py, ob) };
        py_any.to_object(py).into_bound(py)
    }
}

fn unicode_from_str(buf: &str) -> *mut ffi::PyObject {
    let num_chars = bytecount::num_chars(buf.as_bytes());
    match find_str_kind(buf, num_chars) {
        PyUnicodeKind::Ascii => pyunicode_ascii(buf),
        PyUnicodeKind::OneByte => pyunicode_onebyte(buf, num_chars),
        PyUnicodeKind::TwoByte => pyunicode_twobyte(buf, num_chars),
        PyUnicodeKind::FourByte => pyunicode_fourbyte(buf, num_chars),
    }
}

fn find_str_kind(buf: &str, num_chars: usize) -> PyUnicodeKind {
    if buf.len() == num_chars {
        PyUnicodeKind::Ascii
    } else if is_four_byte(buf) {
        PyUnicodeKind::FourByte
    } else if encoding_rs::mem::is_str_latin1(buf) {
        PyUnicodeKind::OneByte
    } else {
        PyUnicodeKind::TwoByte
    }
}

pub fn pyunicode_ascii(buf: &str) -> *mut ffi::PyObject {
    unsafe {
        let ptr = ffi::PyUnicode_New(buf.len() as isize, 127);
        let data_ptr = ptr.cast::<PyASCIIObject>().offset(1) as *mut u8;
        core::ptr::copy_nonoverlapping(buf.as_ptr(), data_ptr, buf.len());
        core::ptr::write(data_ptr.add(buf.len()), 0);
        ptr
    }
}

#[cold]
#[inline(never)]
pub fn pyunicode_onebyte(buf: &str, num_chars: usize) -> *mut ffi::PyObject {
    unsafe {
        let ptr = ffi::PyUnicode_New(num_chars as isize, 255);
        let mut data_ptr = ptr.cast::<PyCompactUnicodeObject>().offset(1) as *mut u8;
        for each in buf.chars().fuse() {
            std::ptr::write(data_ptr, each as u8);
            data_ptr = data_ptr.offset(1);
        }
        core::ptr::write(data_ptr, 0);
        ptr
    }
}

pub fn pyunicode_twobyte(buf: &str, num_chars: usize) -> *mut ffi::PyObject {
    unsafe {
        let ptr = ffi::PyUnicode_New(num_chars as isize, 65535);
        let mut data_ptr = ptr.cast::<PyCompactUnicodeObject>().offset(1) as *mut u16;
        for each in buf.chars().fuse() {
            std::ptr::write(data_ptr, each as u16);
            data_ptr = data_ptr.offset(1);
        }
        core::ptr::write(data_ptr, 0);
        ptr
    }
}

pub fn pyunicode_fourbyte(buf: &str, num_chars: usize) -> *mut ffi::PyObject {
    unsafe {
        let ptr = ffi::PyUnicode_New(num_chars as isize, 1114111);
        let mut data_ptr = ptr.cast::<PyCompactUnicodeObject>().offset(1) as *mut u32;
        for each in buf.chars().fuse() {
            std::ptr::write(data_ptr, each as u32);
            data_ptr = data_ptr.offset(1);
        }
        core::ptr::write(data_ptr, 0);
        ptr
    }
}

const STRIDE_SIZE: usize = 8;

pub fn is_four_byte(buf: &str) -> bool {
    let as_bytes = buf.as_bytes();
    let len = as_bytes.len();
    unsafe {
        let mut idx = 0;
        while idx < len.saturating_sub(STRIDE_SIZE) {
            let mut val: bool = false;
            val |= *as_bytes.get_unchecked(idx) > 239;
            val |= *as_bytes.get_unchecked(idx + 1) > 239;
            val |= *as_bytes.get_unchecked(idx + 2) > 239;
            val |= *as_bytes.get_unchecked(idx + 3) > 239;
            val |= *as_bytes.get_unchecked(idx + 4) > 239;
            val |= *as_bytes.get_unchecked(idx + 5) > 239;
            val |= *as_bytes.get_unchecked(idx + 6) > 239;
            val |= *as_bytes.get_unchecked(idx + 7) > 239;
            idx += STRIDE_SIZE;
            if val {
                return true;
            }
        }
        let mut ret = false;
        while idx < len {
            ret |= *as_bytes.get_unchecked(idx) > 239;
            idx += 1;
        }
        ret
    }
}
