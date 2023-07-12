#![doc = include_str!("../README.md")]
#![feature(core_intrinsics)]

mod decode;
mod element;
mod fleece;
mod parse;
mod value;

use std::fmt;

pub use decode::Decoder;
pub use element::{Element, JsonError, JsonResult};
pub use fleece::{Fleece, FleeceResult, FleeceError, JsonType};
pub use parse::Parser;
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
