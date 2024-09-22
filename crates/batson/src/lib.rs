mod array;
mod decoder;
mod encoder;
mod errors;
pub mod get;
mod header;
mod json_writer;
mod object;

use jiter::JsonValue;

use crate::json_writer::JsonWriter;
use decoder::Decoder;
use encoder::Encoder;
pub use errors::{DecodeErrorType, DecodeResult, EncodeError, EncodeResult, ToJsonError, ToJsonResult};

/// Encode binary data from a JSON value.
///
/// # Errors
///
/// Returns an error if the data is not valid.
pub fn encode_from_json(value: &JsonValue<'_>) -> EncodeResult<Vec<u8>> {
    let mut encoder = Encoder::new();
    encoder.encode_value(value)?;
    encoder.align::<u32>();
    Ok(encoder.into())
}

/// Decode binary data to a JSON value.
///
/// # Errors
///
/// Returns an error if the data is not valid.
pub fn decode_to_json_value(bytes: &[u8]) -> DecodeResult<JsonValue> {
    Decoder::new(bytes).take_value()
}

pub fn batson_to_json_vec(batson_bytes: &[u8]) -> ToJsonResult<Vec<u8>> {
    let mut writer = JsonWriter::new();
    Decoder::new(batson_bytes).write_json(&mut writer)?;
    Ok(writer.into())
}

pub fn batson_to_json_string(batson_bytes: &[u8]) -> ToJsonResult<String> {
    let v = batson_to_json_vec(batson_bytes)?;
    // safe since we're guaranteed to have written valid UTF-8
    unsafe { Ok(String::from_utf8_unchecked(v)) }
}

/// Hack while waiting for <https://github.com/pydantic/jiter/pull/131>
#[must_use]
pub fn compare_json_values(a: &JsonValue<'_>, b: &JsonValue<'_>) -> bool {
    match (a, b) {
        (JsonValue::Null, JsonValue::Null) => true,
        (JsonValue::Bool(a), JsonValue::Bool(b)) => a == b,
        (JsonValue::Int(a), JsonValue::Int(b)) => a == b,
        (JsonValue::BigInt(a), JsonValue::BigInt(b)) => a == b,
        (JsonValue::Float(a), JsonValue::Float(b)) => (a - b).abs() <= f64::EPSILON,
        (JsonValue::Str(a), JsonValue::Str(b)) => a == b,
        (JsonValue::Array(a), JsonValue::Array(b)) => {
            if a.len() != b.len() {
                return false;
            }
            a.iter().zip(b.iter()).all(|(a, b)| compare_json_values(a, b))
        }
        (JsonValue::Object(a), JsonValue::Object(b)) => {
            if a.len() != b.len() {
                return false;
            }
            a.iter()
                .zip(b.iter())
                .all(|((ak, av), (bk, bv))| ak == bk && compare_json_values(av, bv))
        }
        _ => false,
    }
}
