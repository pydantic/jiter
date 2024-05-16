use pyo3::prelude::*;

use jiter::{map_json_error, python_parse, StringCacheMode};

#[pyfunction(
    signature = (
        data,
        *,
        allow_inf_nan=true,
        cache_strings=StringCacheMode::All,
        allow_partial=false,
        catch_duplicate_keys=false
    )
)]
pub fn from_json<'py>(
    py: Python<'py>,
    data: &[u8],
    allow_inf_nan: bool,
    cache_strings: StringCacheMode,
    allow_partial: bool,
    catch_duplicate_keys: bool,
) -> PyResult<Bound<'py, PyAny>> {
    let json_bytes = data;
    python_parse(
        py,
        json_bytes,
        allow_inf_nan,
        cache_strings,
        allow_partial,
        catch_duplicate_keys,
    )
    .map_err(|e| map_json_error(json_bytes, &e))
}

#[pyfunction]
pub fn cache_clear(py: Python<'_>) {
    jiter::cache_clear(py)
}

#[pyfunction]
pub fn cache_usage(py: Python<'_>) -> usize {
    jiter::cache_usage(py)
}

#[pymodule]
#[pyo3(name = "jiter")]
fn jiter_python(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(from_json, m)?)?;
    m.add_function(wrap_pyfunction!(cache_clear, m)?)?;
    m.add_function(wrap_pyfunction!(cache_usage, m)?)?;
    Ok(())
}
