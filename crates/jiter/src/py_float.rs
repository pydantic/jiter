use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::sync::GILOnceCell;
use pyo3::types::PyType;

use crate::Jiter;

#[pyclass]
#[derive(Debug, Clone)]
pub struct JsonFloat(String);

#[pymethods]
impl JsonFloat {
    #[new]
    fn new(raw: String) -> PyResult<Self> {
        let s = Self(raw);
        // check the string is valid by calling `as_float`
        s.as_float()?;
        Ok(s)
    }

    fn as_float(&self) -> PyResult<f64> {
        let bytes = self.0.as_bytes();
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
        decimal.call1((self.0.as_str(),))
    }

    fn __str__(&self) -> &str {
        &self.0
    }

    fn __repr__(&self) -> String {
        format!("JsonFloat({})", self.0)
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
