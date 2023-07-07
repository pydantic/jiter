#![doc = include_str ! ("../README.md")]
#![feature(core_intrinsics)]

mod decode;
mod element;
mod parse;
mod value;

pub use decode::Decoder;
pub use element::{Element, ElementInfo, JsonError, JsonResult};
pub use parse::Parser;
pub use value::{JsonArray, JsonObject, JsonValue};
