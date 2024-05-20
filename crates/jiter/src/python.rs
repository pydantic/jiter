use ahash::AHashSet;
use std::marker::PhantomData;

use pyo3::exceptions::PyValueError;
use pyo3::ffi;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString};

use smallvec::SmallVec;

use crate::errors::{json_err, json_error, JsonError, JsonResult, DEFAULT_RECURSION_LIMIT};
use crate::number_decoder::{NumberAny, NumberInt};
use crate::parse::{Parser, Peek};
use crate::py_string_cache::{StringCacheAll, StringCacheKeys, StringCacheMode, StringMaybeCache, StringNoCache};
use crate::string_decoder::{StringDecoder, Tape};
use crate::JsonErrorType;

/// Parse a JSON value from a byte slice and return a Python object.
///
/// # Arguments
///
/// - `py`: [Python](https://docs.rs/pyo3/latest/pyo3/marker/struct.Python.html) marker token.
/// - `json_data`: The JSON data to parse.
/// - `allow_inf_nan`: Whether to allow `(-)Infinity` and `NaN` values.
/// - `cache_strings`: Whether to cache strings to avoid constructing new Python objects,
/// - `allow_partial`: Whether to allow partial JSON data.
/// - `catch_duplicate_keys`: Whether to catch duplicate keys in objects.
/// this should have a significant improvement on performance but increases memory slightly.
///
/// # Returns
///
/// A [PyObject](https://docs.rs/pyo3/latest/pyo3/type.PyObject.html) representing the parsed JSON value.
pub fn python_parse<'py>(
    py: Python<'py>,
    json_data: &[u8],
    allow_inf_nan: bool,
    cache_mode: StringCacheMode,
    allow_partial: bool,
    catch_duplicate_keys: bool,
) -> JsonResult<Bound<'py, PyAny>> {
    macro_rules! ppp {
        ($string_cache:ident, $key_check:ident) => {
            PythonParser::<$string_cache, $key_check>::parse(py, json_data, allow_inf_nan, allow_partial)
        };
    }

    match cache_mode {
        StringCacheMode::All => match catch_duplicate_keys {
            true => ppp!(StringCacheAll, DuplicateKeyCheck),
            false => ppp!(StringCacheAll, NoopKeyCheck),
        },
        StringCacheMode::Keys => match catch_duplicate_keys {
            true => ppp!(StringCacheKeys, DuplicateKeyCheck),
            false => ppp!(StringCacheKeys, NoopKeyCheck),
        },
        StringCacheMode::None => match catch_duplicate_keys {
            true => ppp!(StringNoCache, DuplicateKeyCheck),
            false => ppp!(StringNoCache, NoopKeyCheck),
        },
    }
}

/// Map a `JsonError` to a `PyErr` which can be raised as an exception in Python as a `ValueError`.
pub fn map_json_error(json_data: &[u8], json_error: &JsonError) -> PyErr {
    PyValueError::new_err(json_error.description(json_data))
}

struct PythonParser<'j, StringCache, KeyCheck> {
    _string_cache: PhantomData<StringCache>,
    _key_check: PhantomData<KeyCheck>,
    parser: Parser<'j>,
    tape: Tape,
    recursion_limit: u8,
    allow_inf_nan: bool,
    allow_partial: bool,
}

impl<'j, StringCache: StringMaybeCache, KeyCheck: MaybeKeyCheck> PythonParser<'j, StringCache, KeyCheck> {
    fn parse<'py>(
        py: Python<'py>,
        json_data: &[u8],
        allow_inf_nan: bool,
        allow_partial: bool,
    ) -> JsonResult<Bound<'py, PyAny>> {
        let mut slf = PythonParser {
            _string_cache: PhantomData::<StringCache>,
            _key_check: PhantomData::<KeyCheck>,
            parser: Parser::new(json_data),
            tape: Tape::default(),
            recursion_limit: DEFAULT_RECURSION_LIMIT,
            allow_inf_nan,
            allow_partial,
        };

        let peek = slf.parser.peek()?;
        let v = slf.py_take_value(py, peek)?;
        if !allow_partial {
            slf.parser.finish()?;
        }
        Ok(v)
    }

    fn py_take_value<'py>(&mut self, py: Python<'py>, peek: Peek) -> JsonResult<Bound<'py, PyAny>> {
        match peek {
            Peek::Null => {
                self.parser.consume_null()?;
                Ok(py.None().into_bound(py))
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
                Ok(StringCache::get_value(py, s.as_str(), s.ascii_only()).into_any())
            }
            Peek::Array => {
                let peek_first = match self.parser.array_first() {
                    Ok(Some(peek)) => peek,
                    Err(e) if !self._allow_partial_err(&e) => return Err(e),
                    Ok(None) | Err(_) => return Ok(PyList::empty_bound(py).into_any()),
                };

                let mut vec: SmallVec<[Bound<'_, PyAny>; 8]> = SmallVec::with_capacity(8);
                if let Err(e) = self._parse_array(py, peek_first, &mut vec) {
                    if !self._allow_partial_err(&e) {
                        return Err(e);
                    }
                }

                Ok(PyList::new_bound(py, vec).into_any())
            }
            Peek::Object => {
                let dict = PyDict::new_bound(py);
                if let Err(e) = self._parse_object(py, &dict) {
                    if !self._allow_partial_err(&e) {
                        return Err(e);
                    }
                }
                Ok(dict.into_any())
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

    fn _parse_array<'py>(
        &mut self,
        py: Python<'py>,
        peek_first: Peek,
        vec: &mut SmallVec<[Bound<'py, PyAny>; 8]>,
    ) -> JsonResult<()> {
        let v = self._check_take_value(py, peek_first)?;
        vec.push(v);
        while let Some(peek) = self.parser.array_step()? {
            let v = self._check_take_value(py, peek)?;
            vec.push(v);
        }
        Ok(())
    }

    fn _parse_object<'py>(&mut self, py: Python<'py>, dict: &Bound<'py, PyDict>) -> JsonResult<()> {
        let set_item = |key: Bound<'py, PyString>, value: Bound<'py, PyAny>| {
            let r = unsafe { ffi::PyDict_SetItem(dict.as_ptr(), key.as_ptr(), value.as_ptr()) };
            // AFAIK this shouldn't happen since the key will always be a string  which is hashable
            // we panic here rather than returning a result and using `?` below as it's up to 14% faster
            // presumably because there are fewer branches
            assert_ne!(r, -1, "PyDict_SetItem failed");
        };
        let mut check_keys = KeyCheck::default();
        if let Some(first_key) = self.parser.object_first::<StringDecoder>(&mut self.tape)? {
            let first_key_s = first_key.as_str();
            check_keys.check(first_key_s, self.parser.index)?;
            let first_key = StringCache::get_key(py, first_key_s, first_key.ascii_only());
            let peek = self.parser.peek()?;
            let first_value = self._check_take_value(py, peek)?;
            set_item(first_key, first_value);
            while let Some(key) = self.parser.object_step::<StringDecoder>(&mut self.tape)? {
                let key_s = key.as_str();
                check_keys.check(key_s, self.parser.index)?;
                let key = StringCache::get_key(py, key_s, key.ascii_only());
                let peek = self.parser.peek()?;
                let value = self._check_take_value(py, peek)?;
                set_item(key, value);
            }
        }
        Ok(())
    }

    fn _allow_partial_err(&self, e: &JsonError) -> bool {
        if self.allow_partial {
            matches!(
                e.error_type,
                JsonErrorType::EofWhileParsingList
                    | JsonErrorType::EofWhileParsingObject
                    | JsonErrorType::EofWhileParsingString
                    | JsonErrorType::EofWhileParsingValue
                    | JsonErrorType::ExpectedListCommaOrEnd
                    | JsonErrorType::ExpectedObjectCommaOrEnd
            )
        } else {
            false
        }
    }

    fn _check_take_value<'py>(&mut self, py: Python<'py>, peek: Peek) -> JsonResult<Bound<'py, PyAny>> {
        self.recursion_limit = match self.recursion_limit.checked_sub(1) {
            Some(limit) => limit,
            None => return json_err!(RecursionLimitExceeded, self.parser.index),
        };

        let r = self.py_take_value(py, peek);

        self.recursion_limit += 1;
        r
    }
}

trait MaybeKeyCheck: Default {
    fn check(&mut self, key: &str, index: usize) -> JsonResult<()>;
}

#[derive(Default)]
struct NoopKeyCheck;

impl MaybeKeyCheck for NoopKeyCheck {
    fn check(&mut self, _key: &str, _index: usize) -> JsonResult<()> {
        Ok(())
    }
}

#[derive(Default)]
struct DuplicateKeyCheck(AHashSet<String>);

impl MaybeKeyCheck for DuplicateKeyCheck {
    fn check(&mut self, key: &str, index: usize) -> JsonResult<()> {
        if self.0.insert(key.to_owned()) {
            Ok(())
        } else {
            Err(JsonError::new(JsonErrorType::DuplicateKey(key.to_owned()), index))
        }
    }
}
