use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::errors::{JsonError, LinePosition};

#[pyclass(extends=PyValueError, module="jiter")]
#[derive(Debug, Clone)]
pub struct JsonParseError {
    json_error: JsonError,
    position: LinePosition,
}

impl JsonParseError {
    pub fn new_err(py: Python, json_error: JsonError, json_data: &[u8]) -> PyErr {
        let position = json_error.get_position(json_data);
        let slf = Self { json_error, position };
        match Py::new(py, slf) {
            Ok(err) => PyErr::from_value_bound(err.into_bound(py).into_any()),
            Err(err) => err,
        }
    }
}

#[pymethods]
impl JsonParseError {
    fn kind(&self) -> &'static str {
        self.json_error.error_type.kind()
    }

    fn description(&self) -> String {
        self.json_error.error_type.to_string()
    }

    fn index(&self) -> usize {
        self.json_error.index
    }

    fn line(&self) -> usize {
        self.position.line
    }

    fn column(&self) -> usize {
        self.position.column
    }

    fn __str__(&self) -> String {
        format!("{} at {}", self.json_error.error_type, self.position)
    }

    fn __repr__(&self) -> String {
        format!("JsonParseError({:?})", self.__str__())
    }
}
