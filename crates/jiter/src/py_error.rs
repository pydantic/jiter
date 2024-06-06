use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::errors::{JsonError, LinePosition};

#[pyclass(extends=PyValueError, module="jiter")]
#[derive(Debug, Clone)]
pub struct JsonParseError {
    json_error: JsonError,
    path: Vec<PathItem>,
    position: LinePosition,
}

impl JsonParseError {
    pub(crate) fn new_err(py: Python, py_json_error: PythonJsonError, json_data: &[u8]) -> PyErr {
        let position = py_json_error.error.get_position(json_data);
        let slf = Self {
            json_error: py_json_error.error,
            path: match py_json_error.path {
                Some(mut v) => {
                    v.reverse();
                    v
                }
                None => vec![],
            },
            position,
        };
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

    fn path(&self, py: Python) -> PyObject {
        self.path.to_object(py)
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

pub(crate) trait MaybeBuildArrayPath: MaybeBuildPath {
    fn incr_index(&mut self);
    fn set_index_path(&self, err: PythonJsonError) -> PythonJsonError;
}

pub(crate) trait MaybeBuildObjectPath: MaybeBuildPath {
    fn set_key(&mut self, key: &str);

    fn set_key_path(&self, err: PythonJsonError) -> PythonJsonError;
}

pub(crate) trait MaybeBuildPath {
    fn new_array() -> impl MaybeBuildArrayPath;
    fn new_object() -> impl MaybeBuildObjectPath;
}

pub(crate) struct NoopBuildPath;

impl MaybeBuildPath for NoopBuildPath {
    fn new_array() -> NoopBuildPath {
        NoopBuildPath
    }

    fn new_object() -> NoopBuildPath {
        NoopBuildPath
    }
}

impl MaybeBuildArrayPath for NoopBuildPath {
    fn incr_index(&mut self) {}

    fn set_index_path(&self, err: PythonJsonError) -> PythonJsonError {
        err
    }
}

impl MaybeBuildObjectPath for NoopBuildPath {
    fn set_key(&mut self, _: &str) {}

    fn set_key_path(&self, err: PythonJsonError) -> PythonJsonError {
        err
    }
}

#[derive(Default)]
pub(crate) struct ActiveBuildPath {
    index: usize,
}

impl MaybeBuildPath for ActiveBuildPath {
    fn new_array() -> ActiveBuildPath {
        ActiveBuildPath::default()
    }

    fn new_object() -> ActiveObjectBuildPath {
        ActiveObjectBuildPath::default()
    }
}

impl MaybeBuildArrayPath for ActiveBuildPath {
    fn incr_index(&mut self) {
        self.index += 1;
    }

    fn set_index_path(&self, mut err: PythonJsonError) -> PythonJsonError {
        err.add(PathItem::Index(self.index));
        err
    }
}

#[derive(Default)]
pub(crate) struct ActiveObjectBuildPath {
    key: String,
}

impl MaybeBuildPath for ActiveObjectBuildPath {
    fn new_array() -> ActiveBuildPath {
        ActiveBuildPath::default()
    }

    fn new_object() -> ActiveObjectBuildPath {
        ActiveObjectBuildPath::default()
    }
}

impl MaybeBuildObjectPath for ActiveObjectBuildPath {
    fn set_key(&mut self, key: &str) {
        self.key = key.to_string();
    }

    fn set_key_path(&self, mut err: PythonJsonError) -> PythonJsonError {
        err.add(PathItem::Key(self.key.clone()));
        err
    }
}

#[derive(Debug, Clone)]
enum PathItem {
    Index(usize),
    Key(String),
}

impl ToPyObject for PathItem {
    fn to_object(&self, py: Python<'_>) -> PyObject {
        match self {
            Self::Index(index) => index.to_object(py),
            Self::Key(str) => str.to_object(py),
        }
    }
}

#[derive(Debug)]
pub struct PythonJsonError {
    pub error: JsonError,
    path: Option<Vec<PathItem>>,
}

pub(crate) type PythonJsonResult<T> = Result<T, PythonJsonError>;

impl From<JsonError> for PythonJsonError {
    fn from(error: JsonError) -> Self {
        Self { error, path: None }
    }
}

impl PythonJsonError {
    fn add(&mut self, path_item: PathItem) {
        match self.path.as_mut() {
            Some(path) => path.push(path_item),
            None => self.path = Some(vec![path_item]),
        }
    }
}
