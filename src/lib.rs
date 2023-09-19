#![doc = include_str!("../README.md")]

mod errors;
mod jiter;
mod lazy_index_map;
mod number_decoder;
mod parse;
mod string_decoder;
mod value;

pub use errors::{
    FilePosition, JiterError, JiterErrorType, JsonError, JsonErrorType, JsonResult, JsonType, JsonValueError,
};
pub use jiter::{Jiter, JiterResult};
pub use lazy_index_map::LazyIndexMap;
pub use number_decoder::{NumberAny, NumberDecoder, NumberDecoderRange, NumberInt};
pub use parse::{Parser, Peak};
pub use string_decoder::{StringDecoder, StringDecoderRange};
pub use value::{JsonArray, JsonObject, JsonValue};
