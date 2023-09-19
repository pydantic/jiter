use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString};

use crate::string_decoder::Tape;
use crate::{FilePosition, JsonError, NumberAny, NumberDecoder, NumberInt, Parser, Peak, StringDecoder};

pub fn python_parse(py: Python, data: &[u8]) -> PyResult<PyObject> {
    let mut parser = Parser::new(data);

    let mje = |e: JsonError| map_json_error(data, e);

    let mut tape = Tape::default();
    let peak = parser.peak().map_err(mje)?;
    let v = py_take_value(py, peak, &mut parser, &mut tape, data)?;
    parser.finish().map_err(mje)?;
    Ok(v)
}

pub(crate) fn py_take_value(
    py: Python,
    peak: Peak,
    parser: &mut Parser,
    tape: &mut Tape,
    data: &[u8],
) -> PyResult<PyObject> {
    let mje = |e: JsonError| map_json_error(data, e);
    match peak {
        Peak::True => {
            parser.consume_true().map_err(mje)?;
            Ok(true.to_object(py))
        }
        Peak::False => {
            parser.consume_false().map_err(mje)?;
            Ok(false.to_object(py))
        }
        Peak::Null => {
            parser.consume_null().map_err(mje)?;
            Ok(py.None())
        }
        Peak::String => {
            let s = parser.consume_string::<StringDecoder>(tape).map_err(mje)?;
            Ok(PyString::new(py, s).to_object(py))
        }
        Peak::Num(first) => {
            let n = parser.consume_number::<NumberDecoder<NumberAny>>(first).map_err(mje)?;
            match n {
                NumberAny::Int(NumberInt::Int(int)) => Ok(int.to_object(py)),
                NumberAny::Int(NumberInt::BigInt(big_int)) => Ok(big_int.to_object(py)),
                NumberAny::Int(NumberInt::Zero) => Ok(0.to_object(py)),
                NumberAny::Float(float) => Ok(float.to_object(py)),
            }
        }
        Peak::Array => {
            // TODO we should create the list with the correct size and insert directly into it
            let mut vec = Vec::new();
            if let Some(peak_first) = parser.array_first().map_err(mje)? {
                let v = py_take_value(py, peak_first, parser, tape, data)?;
                vec.push(v);
                while parser.array_step().map_err(mje)? {
                    let peak = parser.peak().map_err(mje)?;
                    let v = py_take_value(py, peak, parser, tape, data)?;
                    vec.push(v);
                }
            }
            let list = PyList::new(py, vec);
            Ok(list.to_object(py))
        }
        Peak::Object => {
            let dict = PyDict::new(py);
            if let Some(first_key) = parser.object_first::<StringDecoder>(tape).map_err(mje)? {
                let first_key = PyString::new(py, first_key);
                let peak = parser.peak().map_err(mje)?;
                let first_value = py_take_value(py, peak, parser, tape, data)?;
                dict.set_item(first_key, first_value)?;
                while let Some(key) = parser.object_step::<StringDecoder>(tape).map_err(mje)? {
                    let key = PyString::new(py, key);
                    let peak = parser.peak().map_err(mje)?;
                    let value = py_take_value(py, peak, parser, tape, data)?;
                    dict.set_item(key, value)?;
                }
            }

            Ok(dict.to_object(py))
        }
    }
}

fn map_json_error(data: &[u8], json_error: JsonError) -> PyErr {
    let JsonError { error_type, index } = json_error;
    let position = FilePosition::find(data, index);
    let msg = format!("{} at {}", error_type, position);
    PyValueError::new_err(msg)
}
