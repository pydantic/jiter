use std::sync::OnceLock;

use pyo3::prelude::*;

use jiter::{map_json_error, LosslessFloat, PartialMode, PythonParse, StringCacheMode};

#[allow(clippy::fn_params_excessive_bools)]
#[pyfunction(
    signature = (
        json_data,
        /,
        *,
        allow_inf_nan=true,
        cache_mode=StringCacheMode::All,
        partial_mode=PartialMode::Off,
        catch_duplicate_keys=false,
        lossless_floats=false,
    )
)]
pub fn from_json<'py>(
    py: Python<'py>,
    json_data: &[u8],
    allow_inf_nan: bool,
    cache_mode: StringCacheMode,
    partial_mode: PartialMode,
    catch_duplicate_keys: bool,
    lossless_floats: bool,
) -> PyResult<Bound<'py, PyAny>> {
    let parse_builder = PythonParse {
        allow_inf_nan,
        cache_mode,
        partial_mode,
        catch_duplicate_keys,
        lossless_floats,
    };
    parse_builder
        .python_parse(py, json_data)
        .map_err(|e| map_json_error(json_data, &e))
}

pub fn get_jiter_version() -> &'static str {
    static JITER_VERSION: OnceLock<String> = OnceLock::new();

    JITER_VERSION.get_or_init(|| {
        let version = env!("CARGO_PKG_VERSION");
        // cargo uses "1.0-alpha1" etc. while python uses "1.0.0a1", this is not full compatibility,
        // but it's good enough for now
        // see https://docs.rs/semver/1.0.9/semver/struct.Version.html#method.parse for rust spec
        // see https://peps.python.org/pep-0440/ for python spec
        // it seems the dot after "alpha/beta" e.g. "-alpha.1" is not necessary, hence why this works
        version.replace("-alpha", "a").replace("-beta", "b")
    })
}

#[pyfunction]
pub fn cache_clear(py: Python<'_>) {
    jiter::cache_clear(py);
}

#[pyfunction]
pub fn cache_usage(py: Python<'_>) -> usize {
    jiter::cache_usage(py)
}

#[pymodule]
#[pyo3(name = "jiter")]
fn jiter_python(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", get_jiter_version())?;
    m.add_function(wrap_pyfunction!(from_json, m)?)?;
    m.add_function(wrap_pyfunction!(cache_clear, m)?)?;
    m.add_function(wrap_pyfunction!(cache_usage, m)?)?;
    m.add_class::<LosslessFloat>()?;
    Ok(())
}
