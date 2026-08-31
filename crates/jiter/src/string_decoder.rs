use std::ops::Range;
use std::str::{from_utf8, from_utf8_unchecked};

use crate::errors::{JsonErrorType, JsonResult, json_err, json_error};
use crate::simd::decode_string_chunk;

pub type Tape = Vec<u8>;

/// `'t` is the lifetime of the tape (reusable buffer), `'j` is the lifetime of the JSON data itself
/// data must outlive tape, so if you return data with the lifetime of tape,
/// a slice of data the original JSON data is okay too
pub trait AbstractStringDecoder<'t, 'j>
where
    'j: 't,
{
    type Output: std::fmt::Debug;

    fn decode(
        data: &'j [u8],
        index: usize,
        tape: &'t mut Tape,
        allow_partial: bool,
    ) -> JsonResult<(Self::Output, usize)>;
}

pub struct StringDecoder;

#[derive(Debug)]
pub enum StringOutputType<'t, 'j>
where
    'j: 't,
{
    Tape(&'t str),
    Data(&'j str),
}

/// This submodule is used to create a safety boundary where the `ascii_only`
/// flag can be used to carry soundness information about the string.
mod string_output {
    use std::borrow::Cow;

    use super::StringOutputType;

    #[derive(Debug)]
    pub struct StringOutput<'t, 'j>
    where
        'j: 't,
    {
        pub(crate) data: StringOutputType<'t, 'j>,
        // SAFETY: this is used as an invariant to determine if the string is ascii only
        // so this should not be set except when known
        ascii_only: bool,
    }

    impl From<StringOutput<'_, '_>> for String {
        fn from(val: StringOutput) -> Self {
            match val.data {
                StringOutputType::Tape(s) | StringOutputType::Data(s) => s.to_owned(),
            }
        }
    }

    impl<'j> From<StringOutput<'_, 'j>> for Cow<'j, str> {
        fn from(val: StringOutput<'_, 'j>) -> Self {
            match val.data {
                StringOutputType::Tape(s) => s.to_owned().into(),
                StringOutputType::Data(s) => s.into(),
            }
        }
    }

    impl<'t, 'j> StringOutput<'t, 'j>
    where
        'j: 't,
    {
        /// # Safety
        ///
        /// `ascii_only` must only be set to true if the string is ASCII only
        pub unsafe fn tape(data: &'t str, ascii_only: bool) -> Self {
            StringOutput {
                data: StringOutputType::Tape(data),
                ascii_only,
            }
        }

        /// # Safety
        ///
        /// `ascii_only` must only be set to true if the string is ASCII only
        pub unsafe fn data(data: &'j str, ascii_only: bool) -> Self {
            StringOutput {
                data: StringOutputType::Data(data),
                ascii_only,
            }
        }

        pub fn as_str(&self) -> &'t str {
            match self.data {
                StringOutputType::Tape(s) | StringOutputType::Data(s) => s,
            }
        }

        pub fn ascii_only(&self) -> bool {
            self.ascii_only
        }
    }
}

pub use string_output::StringOutput;

impl<'t, 'j> AbstractStringDecoder<'t, 'j> for StringDecoder
where
    'j: 't,
{
    type Output = StringOutput<'t, 'j>;

    fn decode(
        data: &'j [u8],
        index: usize,
        tape: &'t mut Tape,
        allow_partial: bool,
    ) -> JsonResult<(Self::Output, usize)> {
        let start = index + 1;

        match decode_string_chunk(data, start, true, allow_partial)? {
            (StringChunk::StringEnd, ascii_only, index) => {
                let s = to_str(&data[start..index], ascii_only, start, allow_partial)?;
                // SAFETY: `ascii_only` tracks whether the decoded string contains only ASCII.
                Ok((unsafe { StringOutput::data(s, ascii_only) }, index + 1))
            }
            (StringChunk::Backslash, ascii_only, index) => {
                decode_to_tape(data, index, tape, start, ascii_only, allow_partial)
            }
        }
    }
}

fn decode_to_tape<'t, 'j>(
    data: &'j [u8],
    mut index: usize,
    tape: &'t mut Tape,
    start: usize,
    mut ascii_only: bool,
    allow_partial: bool,
) -> JsonResult<(StringOutput<'t, 'j>, usize)> {
    tape.clear();
    let mut chunk_start = start;
    loop {
        // on_backslash
        tape.extend_from_slice(&data[chunk_start..index]);
        index += 1;
        if let Some(next_inner) = data.get(index) {
            match next_inner {
                b'"' | b'\\' | b'/' => tape.push(*next_inner),
                b'b' => tape.push(b'\x08'),
                b'f' => tape.push(b'\x0C'),
                b'n' => tape.push(b'\n'),
                b'r' => tape.push(b'\r'),
                b't' => tape.push(b'\t'),
                b'u' => match parse_escape(data, index) {
                    Ok((c, new_index)) => {
                        ascii_only = false;
                        index = new_index;
                        tape.extend_from_slice(c.encode_utf8(&mut [0_u8; 4]).as_bytes());
                    }
                    Err(e) => {
                        if allow_partial && e.error_type == JsonErrorType::EofWhileParsingString {
                            let s = to_str(tape, ascii_only, start, allow_partial)?;
                            // SAFETY: `ascii_only` tracks whether the decoded string contains only ASCII.
                            return Ok((unsafe { StringOutput::tape(s, ascii_only) }, e.index));
                        }
                        return Err(e);
                    }
                },
                _ => return json_err!(InvalidEscape, index),
            }
            index += 1;
        } else {
            if allow_partial {
                let s = to_str(tape, ascii_only, start, allow_partial)?;
                // SAFETY: `ascii_only` tracks whether the decoded string contains only ASCII.
                return Ok((unsafe { StringOutput::tape(s, ascii_only) }, index));
            }
            return json_err!(EofWhileParsingString, index);
        }

        match decode_string_chunk(data, index, ascii_only, allow_partial)? {
            (StringChunk::StringEnd, ascii_only, new_index) => {
                tape.extend_from_slice(&data[index..new_index]);
                index = new_index + 1;
                let s = to_str(tape, ascii_only, start, allow_partial)?;
                // SAFETY: `ascii_only` tracks whether the decoded string contains only ASCII.
                return Ok((unsafe { StringOutput::tape(s, ascii_only) }, index));
            }
            (StringChunk::Backslash, ascii_only_new, index_new) => {
                ascii_only = ascii_only_new;
                chunk_start = index;
                index = index_new;
            }
        }
    }
}

pub(crate) enum StringChunk {
    StringEnd,
    Backslash,
}

fn to_str(bytes: &[u8], ascii_only: bool, start: usize, allow_partial: bool) -> JsonResult<&str> {
    if ascii_only {
        // safety: in this case we've already confirmed that all characters are ascii, we can safely
        // transmute from bytes to str
        Ok(unsafe { from_utf8_unchecked(bytes) })
    } else {
        match from_utf8(bytes) {
            Ok(s) => Ok(s),
            Err(e) if allow_partial && e.error_len().is_none() => {
                // In partial mode, we handle incomplete (not invalid) UTF-8 sequences
                // by truncating to the last valid UTF-8 boundary
                // (`error_len()` is `None` for incomplete sequences)
                let valid_up_to = e.valid_up_to();
                // SAFETY: `valid_up_to()` returns the byte index up to which the input is valid UTF-8
                Ok(unsafe { from_utf8_unchecked(&bytes[..valid_up_to]) })
            }
            Err(e) => Err(json_error!(InvalidUnicodeCodePoint, start + e.valid_up_to() + 1)),
        }
    }
}

/// Taken approximately from https://github.com/serde-rs/json/blob/v1.0.107/src/read.rs#L872-L945
fn parse_escape(data: &[u8], index: usize) -> JsonResult<(char, usize)> {
    let (n, index) = parse_u4(data, index)?;
    match n {
        0xDC00..=0xDFFF => json_err!(LoneLeadingSurrogateInHexEscape, index),
        0xD800..=0xDBFF => match data.get(index + 1..index + 3) {
            Some(slice) if slice == b"\\u" => {
                let (n2, index) = parse_u4(data, index + 2)?;
                if !(0xDC00..=0xDFFF).contains(&n2) {
                    return json_err!(LoneLeadingSurrogateInHexEscape, index);
                }
                let n2 = ((((n - 0xD800) as u32) << 10) | ((n2 - 0xDC00) as u32)) + 0x1_0000;

                match char::from_u32(n2) {
                    Some(c) => Ok((c, index)),
                    None => json_err!(EofWhileParsingString, index),
                }
            }
            Some(slice) if slice.starts_with(b"\\") => json_err!(UnexpectedEndOfHexEscape, index + 2),
            Some(_) => json_err!(UnexpectedEndOfHexEscape, index + 1),
            None => match data.get(index + 1) {
                Some(b'\\') | None => json_err!(EofWhileParsingString, data.len()),
                Some(_) => json_err!(UnexpectedEndOfHexEscape, index + 1),
            },
        },
        _ => match char::from_u32(n as u32) {
            Some(c) => Ok((c, index)),
            None => json_err!(InvalidEscape, index),
        },
    }
}

fn parse_u4(data: &[u8], mut index: usize) -> JsonResult<(u16, usize)> {
    let mut n = 0;
    let u4 = data
        .get(index + 1..index + 5)
        .ok_or_else(|| json_error!(EofWhileParsingString, data.len()))?;

    for c in u4 {
        index += 1;
        let hex = match c {
            b'0'..=b'9' => (c & 0x0f) as u16,
            b'a'..=b'f' => (c - b'a' + 10) as u16,
            b'A'..=b'F' => (c - b'A' + 10) as u16,
            _ => return json_err!(InvalidEscape, index),
        };
        n = (n << 4) + hex;
    }
    Ok((n, index))
}

/// A string decoder that returns the range of the string.
///
/// *WARNING:* For performance reasons, this decoder does not check that the string would be valid UTF-8.
pub struct StringDecoderRange;

impl<'t, 'j> AbstractStringDecoder<'t, 'j> for StringDecoderRange
where
    'j: 't,
{
    type Output = Range<usize>;

    fn decode(
        data: &'j [u8],
        mut index: usize,
        _tape: &'t mut Tape,
        allow_partial: bool,
    ) -> JsonResult<(Self::Output, usize)> {
        index += 1;
        let start = index;

        loop {
            index = match decode_string_chunk(data, index, true, allow_partial)? {
                (StringChunk::StringEnd, _, index) => {
                    let r = start..index;
                    return Ok((r, index + 1));
                }
                (StringChunk::Backslash, _, index) => index,
            };
            index += 1;
            if let Some(next_inner) = data.get(index) {
                match next_inner {
                    // these escapes are easy to validate
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => (),
                    b'u' => match parse_escape(data, index) {
                        Ok((_, new_index)) => index = new_index,
                        // input ends inside the `\u` escape, keep the span before the backslash
                        Err(e) if allow_partial && e.error_type == JsonErrorType::EofWhileParsingString => {
                            return Ok((start..index - 1, e.index));
                        }
                        Err(e) => return Err(e),
                    },
                    _ => return json_err!(InvalidEscape, index),
                }
                index += 1;
            } else if allow_partial {
                // input ends right after the backslash, keep the span before it
                return Ok((start..index - 1, index));
            } else {
                return json_err!(EofWhileParsingString, index);
            }
        }
    }
}
