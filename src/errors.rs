use std::fmt;

/// Enum representing all possible errors in JSON syntax.
///
/// Almost all of `JsonErrorType` is copied from [serde_json](https://github.com/serde-rs) so errors match
/// those expected from `serde_json`.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum JsonErrorType {
    /// string escape sequences are not supported in this method, usize here is the position within the string
    /// that is invalid
    StringEscapeNotSupported,

    /// float value was found where an int was expected
    FloatExpectingInt,

    /// NOTE: all errors from here on are copied from serde_json
    /// [src/error.rs](https://github.com/serde-rs/json/blob/v1.0.107/src/error.rs#L236)
    /// with `Io` and `Message` removed
    ///
    /// EOF while parsing a list.
    EofWhileParsingList,

    /// EOF while parsing an object.
    EofWhileParsingObject,

    /// EOF while parsing a string.
    EofWhileParsingString,

    /// EOF while parsing a JSON value.
    EofWhileParsingValue,

    /// Expected this character to be a `':'`.
    ExpectedColon,

    /// Expected this character to be either a `','` or a `']'`.
    ExpectedListCommaOrEnd,

    /// Expected this character to be either a `','` or a `'}'`.
    ExpectedObjectCommaOrEnd,

    /// Expected to parse either a `true`, `false`, or a `null`.
    ExpectedSomeIdent,

    /// Expected this character to start a JSON value.
    ExpectedSomeValue,

    /// Invalid hex escape code.
    InvalidEscape,

    /// Invalid number.
    InvalidNumber,

    /// Number is bigger than the maximum value of its type.
    NumberOutOfRange,

    /// Invalid unicode code point.
    InvalidUnicodeCodePoint,

    /// Control character found while parsing a string.
    ControlCharacterWhileParsingString,

    /// Object key is not a string.
    KeyMustBeAString,

    /// Lone leading surrogate in hex escape.
    LoneLeadingSurrogateInHexEscape,

    /// JSON has a comma after the last value in an array or map.
    TrailingComma,

    /// JSON has non-whitespace trailing characters after the value.
    TrailingCharacters,

    /// Unexpected end of hex escape.
    UnexpectedEndOfHexEscape,

    /// Encountered nesting of JSON maps and arrays more than 128 layers deep.
    RecursionLimitExceeded,
}

impl std::fmt::Display for JsonErrorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Messages for enum members copied from serde_json are unchanged
        match self {
            Self::StringEscapeNotSupported => f.write_str("string escape sequences are not supported"),
            Self::FloatExpectingInt => f.write_str("float value was found where an int was expected"),
            Self::EofWhileParsingList => f.write_str("EOF while parsing a list"),
            Self::EofWhileParsingObject => f.write_str("EOF while parsing an object"),
            Self::EofWhileParsingString => f.write_str("EOF while parsing a string"),
            Self::EofWhileParsingValue => f.write_str("EOF while parsing a value"),
            Self::ExpectedColon => f.write_str("expected `:`"),
            Self::ExpectedListCommaOrEnd => f.write_str("expected `,` or `]`"),
            Self::ExpectedObjectCommaOrEnd => f.write_str("expected `,` or `}`"),
            Self::ExpectedSomeIdent => f.write_str("expected ident"),
            Self::ExpectedSomeValue => f.write_str("expected value"),
            Self::InvalidEscape => f.write_str("invalid escape"),
            Self::InvalidNumber => f.write_str("invalid number"),
            Self::NumberOutOfRange => f.write_str("number out of range"),
            Self::InvalidUnicodeCodePoint => f.write_str("invalid unicode code point"),
            Self::ControlCharacterWhileParsingString => {
                f.write_str("control character (\\u0000-\\u001F) found while parsing a string")
            }
            Self::KeyMustBeAString => f.write_str("key must be a string"),
            Self::LoneLeadingSurrogateInHexEscape => f.write_str("lone leading surrogate in hex escape"),
            Self::TrailingComma => f.write_str("trailing comma"),
            Self::TrailingCharacters => f.write_str("trailing characters"),
            Self::UnexpectedEndOfHexEscape => f.write_str("unexpected end of hex escape"),
            Self::RecursionLimitExceeded => f.write_str("recursion limit exceeded"),
        }
    }
}

pub type JsonResult<T> = Result<T, JsonError>;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct JsonError {
    pub error_type: JsonErrorType,
    pub index: usize,
}

impl JsonError {
    pub(crate) fn new(error_type: JsonErrorType, index: usize) -> Self {
        Self { error_type, index }
    }
}

impl std::fmt::Display for JsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at index {}", self.error_type, self.index)
    }
}

macro_rules! json_error {
    ($error_type:ident, $index:expr) => {
        crate::errors::JsonError::new(crate::errors::JsonErrorType::$error_type, $index)
    };
}

pub(crate) use json_error;

macro_rules! json_err {
    ($error_type:ident, $index:expr) => {
        Err(crate::errors::json_error!($error_type, $index))
    };
}

use crate::Jiter;
pub(crate) use json_err;

pub(crate) const DEFAULT_RECURSION_LIMIT: u8 = 200;

/// Enum representing all JSON types.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum JsonType {
    Null,
    Bool,
    Int,
    Float,
    String,
    Array,
    Object,
}

impl std::fmt::Display for JsonType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Null => f.write_str("null"),
            Self::Bool => f.write_str("bool"),
            Self::Int => f.write_str("int"),
            Self::Float => f.write_str("float"),
            Self::String => f.write_str("string"),
            Self::Array => f.write_str("array"),
            Self::Object => f.write_str("object"),
        }
    }
}

/// Enum representing either a [JsonErrorType] or a WrongType error.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum JiterErrorType {
    JsonError(JsonErrorType),
    WrongType { expected: JsonType, actual: JsonType },
}

impl std::fmt::Display for JiterErrorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::JsonError(error_type) => write!(f, "{}", error_type),
            Self::WrongType { expected, actual } => {
                write!(f, "expected {} but found {}", expected, actual)
            }
        }
    }
}

/// An error from the Jiter iterator.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct JiterError {
    pub error_type: JiterErrorType,
    pub index: usize,
    pub position: Option<FilePosition>,
}

impl std::fmt::Display for JiterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(ref p) = self.position {
            write!(f, "{} at {}", self.error_type, p)
        } else {
            write!(f, "{} at index {}", self.error_type, self.index)
        }
    }
}

impl JiterError {
    pub(crate) fn new(error_type: JiterErrorType, index: usize) -> Self {
        Self {
            error_type,
            index,
            position: None,
        }
    }

    pub fn with_position(&self, jiter: &Jiter) -> Self {
        Self {
            error_type: self.error_type.clone(),
            index: self.index,
            position: Some(jiter.error_position(self.index)),
        }
    }

    pub(crate) fn wrong_type(expected: JsonType, actual: JsonType, index: usize) -> Self {
        Self::new(JiterErrorType::WrongType { expected, actual }, index)
    }
}

impl From<JsonError> for JiterError {
    fn from(error: JsonError) -> Self {
        Self {
            error_type: JiterErrorType::JsonError(error.error_type),
            index: error.index,
            position: None,
        }
    }
}

/// An error from the [crate::JsonValue::parse] method.
#[derive(Debug, Clone)]
pub struct JsonValueError {
    /// The type of error.
    pub error_type: JsonErrorType,
    /// The index in the data where the error occurred.
    pub index: usize,
    /// The line and column in the data where the error occurred.
    pub position: FilePosition,
}

impl std::fmt::Display for JsonValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {}", self.error_type, self.position)
    }
}

impl JsonValueError {
    pub(crate) fn new(error_type: JsonErrorType, index: usize, position: FilePosition) -> Self {
        Self {
            error_type,
            index,
            position,
        }
    }
}

/// Represents a line and column in a file, used for both errors and value positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePosition {
    /// Line number, starting at 1.
    pub line: usize,
    /// Column number, starting at 1.
    pub column: usize,
}

impl fmt::Display for FilePosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {} column {}", self.line, self.column)
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
            if *next == b'\n' {
                line += 1;
                last_line_start = index + 1;
            }
            if index == find {
                return Self {
                    line,
                    column: index + 1 - last_line_start,
                };
            }
            index += 1;
        }
        Self {
            line,
            column: index.saturating_sub(last_line_start),
        }
    }

    pub fn short(&self) -> String {
        format!("{}:{}", self.line, self.column)
    }
}
