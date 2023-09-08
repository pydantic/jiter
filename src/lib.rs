#![doc = include_str!("../README.md")]
#![feature(core_intrinsics)]

mod fleece;
mod number_decoder;
mod parse;
mod string_decoder;
mod value;

use std::fmt;

pub use fleece::{Fleece, FleeceError, FleeceResult, JsonType};
pub use parse::{JsonError, JsonResult, Parser, Peak};
pub use string_decoder::{StringDecoderRange, StringDecoder};
pub use number_decoder::{NumberDecoder, NumberInt, NumberAny, NumberDecoderRange};
pub use value::{JsonArray, JsonObject, JsonValue};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePosition {
    pub line: usize,
    pub column: usize,
}

impl fmt::Display for FilePosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

impl FilePosition {
    pub fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }

    /// Find the line and column of a byte index in a string.
    pub fn find(data: &[u8], find: usize) -> Self {
        let mut line = 1;
        let mut last_line_start = 0;
        let mut index = 0;
        while let Some(next) = data.get(index) {
            if index == find {
                break;
            } else if *next == b'\n' {
                line += 1;
                last_line_start = index + 1;
            }
            index += 1;
        }
        Self {
            line,
            column: index - last_line_start + 1,
        }
    }
}
