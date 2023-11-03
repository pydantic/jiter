#![doc = include_str!("../README.md")]

mod errors;
mod jiter;
mod lazy_index_map;
mod number_decoder;
mod parse;
#[cfg(feature = "python")]
mod python;
mod string_decoder;
mod value;

pub use errors::{FilePosition, JiterError, JiterErrorType, JsonErrorType, JsonResult, JsonType, JsonValueError};
pub use jiter::{Jiter, JiterResult};
pub use lazy_index_map::LazyIndexMap;
pub use number_decoder::{NumberAny, NumberInt};
pub use parse::Peak;
pub use value::{JsonArray, JsonObject, JsonValue};

#[cfg(feature = "python")]
pub use python::python_parse;
