use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::sync::GILOnceCell;
use pyo3::types::PyType;

use crate::Jiter;

/// Represents a float from JSON, by holding the underlying bytes representing a float from JSON.
#[derive(Debug, Clone)]
#[pyclass]
pub struct LosslessFloat(Vec<u8>);

impl From<Vec<u8>> for LosslessFloat {
    fn from(raw: Vec<u8>) -> Self {
        Self(raw)
    }
}

#[pymethods]
impl LosslessFloat {
    #[new]
    fn new(raw: Vec<u8>) -> PyResult<Self> {
        let s = Self(raw);
        // check the string is valid by calling `as_float`
        s.as_float()?;
        Ok(s)
    }

    fn as_float(&self) -> PyResult<f64> {
        let bytes = &self.0;
        let mut jiter = Jiter::new(bytes, true);
        let f = jiter
            .next_float()
            .map_err(|e| PyValueError::new_err(e.description(&jiter)))?;
        jiter
            .finish()
            .map_err(|e| PyValueError::new_err(e.description(&jiter)))?;
        Ok(f)
    }

    fn as_decimal<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let decimal = get_decimal_type(py)?;
        let float_str = self.__str__()?;
        decimal.call1((float_str,))
    }

    fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    fn __str__(&self) -> PyResult<&str> {
        std::str::from_utf8(&self.0).map_err(|_| PyValueError::new_err("Invalid UTF-8"))
    }

    fn __repr__(&self) -> PyResult<String> {
        self.__str__().map(|s| format!("LosslessFloat({s})"))
    }
}

static DECIMAL_TYPE: GILOnceCell<Py<PyType>> = GILOnceCell::new();

pub fn get_decimal_type(py: Python) -> PyResult<&Bound<'_, PyType>> {
    DECIMAL_TYPE
        .get_or_try_init(py, || {
            py.import_bound("decimal")?
                .getattr("Decimal")?
                .extract::<&PyType>()
                .map(Into::into)
        })
        .map(|t| t.bind(py))
}
