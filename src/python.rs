use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString};

use smallvec::SmallVec;

use crate::errors::{json_error, FilePosition, JsonError, DEFAULT_RECURSION_LIMIT};
use crate::number_decoder::{NumberAny, NumberInt};
use crate::parse::{Parser, Peak};
use crate::string_decoder::{StringDecoder, Tape};

/// Parse a JSON value from a byte slice and return a Python object.
///
/// # Arguments
/// - `py`: [Python](https://docs.rs/pyo3/latest/pyo3/marker/struct.Python.html) marker token.
/// - `json_data`: The JSON data to parse.
/// - `allow_inf_nan`: Whether to allow `(-)Infinity` and `NaN` values.
///
/// # Returns
///
/// A [PyObject](https://docs.rs/pyo3/latest/pyo3/type.PyObject.html) representing the parsed JSON value.
pub fn python_parse(py: Python, json_data: &[u8], allow_inf_nan: bool) -> PyResult<PyObject> {
    let mut python_parser = PythonParser {
        parser: Parser::new(json_data),
        tape: Tape::default(),
        data: json_data,
        recursion_limit: DEFAULT_RECURSION_LIMIT,
        allow_inf_nan,
    };

    let mje = |e: JsonError| map_json_error(json_data, e);

    let peak = python_parser.parser.peak().map_err(mje)?;
    let v = python_parser.py_take_value(py, peak)?;
    python_parser.parser.finish().map_err(mje)?;
    Ok(v)
}

struct PythonParser<'j> {
    parser: Parser<'j>,
    tape: Tape,
    data: &'j [u8],
    recursion_limit: u8,
    allow_inf_nan: bool,
}

impl<'j> PythonParser<'j> {
    fn py_take_value(&mut self, py: Python, peak: Peak) -> PyResult<PyObject> {
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
                let n = self
                    .parser
                    .consume_number::<NumberAny>(first, self.allow_inf_nan)
                    .map_err(mje)?;
                match n {
                    NumberAny::Int(NumberInt::Int(int)) => Ok(int.to_object(py)),
                    NumberAny::Int(NumberInt::BigInt(big_int)) => Ok(big_int.to_object(py)),
                    NumberAny::Float(float) => Ok(float.to_object(py)),
                }
            }
            Peak::Array => {
                let list = if let Some(peak_first) = self.parser.array_first().map_err(mje)? {
                    let mut vec: SmallVec<[PyObject; 8]> = SmallVec::with_capacity(8);
                    let v = self._check_take_value(py, peak_first)?;
                    vec.push(v);
                    while let Some(peak) = self.parser.array_step().map_err(mje)? {
                        let v = self._check_take_value(py, peak)?;
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
                if let Some(first_key) = self.parser.object_first::<StringDecoder>(&mut self.tape).map_err(mje)? {
                    let first_key = PyString::new(py, first_key.as_str());
                    let peak = self.parser.peak().map_err(mje)?;
                    let first_value = self._check_take_value(py, peak)?;
                    dict.set_item(first_key, first_value)?;
                    while let Some(key) = self.parser.object_step::<StringDecoder>(&mut self.tape).map_err(mje)? {
                        let key = PyString::new(py, key.as_str());
                        let peak = self.parser.peak().map_err(mje)?;
                        let value = self._check_take_value(py, peak)?;
                        dict.set_item(key, value)?;
                    }
                }
                Ok(dict.to_object(py))
            }
        }
    }

    fn _check_take_value(&mut self, py: Python, peak: Peak) -> PyResult<PyObject> {
        self.recursion_limit = match self.recursion_limit.checked_sub(1) {
            Some(limit) => limit,
            None => {
                return Err(map_json_error(
                    self.data,
                    json_error!(RecursionLimitExceeded, self.parser.index),
                ))
            }
        };

        let r = self.py_take_value(py, peak);

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
