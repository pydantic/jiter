use crate::map_json_error;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::number_decoder::{AbstractNumberDecoder, NumberAny};

#[pyclass]
#[derive(Debug, Clone)]
struct JsonFloat(String);

#[pymethods]
impl JsonFloat {
    #[new]
    fn new(raw: String) -> Self {
        Self(raw)
    }

    fn as_float(&self) -> PyResult<f64> {
        let bytes = self.0.as_bytes();
        if let Some(first) = bytes.first() {
            let (n, _) = NumberAny::decode(bytes, 0, *first, true).map_err(|e| map_json_error(bytes, &e))?;
            match n {
                NumberAny::Float(f) => Ok(f),
                NumberAny::Int(int) => Ok(int.into()),
            }
        } else {
            Err(PyValueError::new_err("empty string is not a valid float"))
        }
    }

    fn __str__(&self) -> &str {
        &self.0
    }

    fn __repr__(&self) -> String {
        format!("JsonFloat({:?})", self.0)
    }
}
