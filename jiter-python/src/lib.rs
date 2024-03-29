use pyo3::prelude::*;

use jiter::{map_json_error, python_parse};

#[pyfunction(signature = (data, *, allow_inf_nan=true, cache_strings=true))]
pub fn from_json<'py>(
    py: Python<'py>,
    data: &[u8],
    allow_inf_nan: bool,
    cache_strings: bool,
) -> PyResult<Bound<'py, PyAny>> {
    let cache_mode = cache_strings.into();
    let json_bytes = data;
    python_parse(py, json_bytes, allow_inf_nan, cache_mode, false).map_err(|e| map_json_error(json_bytes, &e))
}

#[pymodule]
fn jiter_python(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(from_json, m)?)?;
    Ok(())
}
