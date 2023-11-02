use std::fmt::Display;
use std::ops::Range;

use crate::errors::{json_err, json_error, JsonResult};

pub type Tape = Vec<u8>;

/// `'t` is the lifetime of the tape (reusable buffer), `'j` is the lifetime of the JSON data itself
/// data must outlive tape, so if you return data with the lifetime of tape,
/// a slice of data the original JSON data is okay too
pub trait AbstractStringDecoder<'t, 'j>
where
    'j: 't,
{
    type Output;

    fn decode(data: &'j [u8], index: usize, tape: &'t mut Tape) -> JsonResult<(Self::Output, usize)>;
}

pub struct StringDecoder;

#[derive(Debug)]
pub enum StringOutput<'t, 'j>
where
    'j: 't,
{
    Tape(&'t str),
    Data(&'j str),
}

impl Display for StringOutput<'_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Tape(s) => write!(f, "{}", s),
            Self::Data(s) => write!(f, "{}", s),
        }
    }
}

impl<'t, 'j> StringOutput<'t, 'j> {
    pub fn as_str(&self) -> &'t str {
        match self {
            Self::Tape(s) => s,
            Self::Data(s) => s,
        }
    }
}

impl<'t, 'j> AbstractStringDecoder<'t, 'j> for StringDecoder
where
    'j: 't,
{
    type Output = StringOutput<'t, 'j>;

    fn decode(data: &'j [u8], mut index: usize, tape: &'t mut Tape) -> JsonResult<(Self::Output, usize)> {
        index += 1;
        tape.clear();
        let start = index;
        let mut last_escape = start;
        let mut found_escape = false;
        let mut ascii_only = true;

        while let Some(next) = data.get(index) {
            match next {
                b'"' => {
                    return if found_escape {
                        tape.extend_from_slice(&data[last_escape..index]);
                        index += 1;
                        let s = to_str(tape, ascii_only, start)?;
                        Ok((StringOutput::Tape(s), index))
                    } else {
                        let s = to_str(&data[start..index], ascii_only, start)?;
                        index += 1;
                        Ok((StringOutput::Data(s), index))
                    };
                }
                b'\\' => {
                    found_escape = true;
                    tape.extend_from_slice(&data[last_escape..index]);
                    index += 1;
                    if let Some(next_inner) = data.get(index) {
                        match next_inner {
                            b'"' | b'\\' | b'/' => tape.push(*next_inner),
                            b'b' => tape.push(b'\x08'),
                            b'f' => tape.push(b'\x0C'),
                            b'n' => tape.push(b'\n'),
                            b'r' => tape.push(b'\r'),
                            b't' => tape.push(b'\t'),
                            b'u' => {
                                let (c, new_index) = parse_escape(data, index)?;
                                index = new_index;
                                tape.extend_from_slice(c.encode_utf8(&mut [0_u8; 4]).as_bytes());
                            }
                            _ => return json_err!(InvalidEscape, index),
                        }
                        last_escape = index + 1;
                    } else {
                        break;
                    }
                }
                // all values below 32 are invalid
                next if *next < 32u8 => return json_err!(ControlCharacterWhileParsingString, index),
                next if *next >= 128u8 && ascii_only => {
                    ascii_only = false;
                }
                _ => (),
            }
            index += 1;
        }
        json_err!(EofWhileParsingString, index)
    }
}

fn to_str(bytes: &[u8], ascii_only: bool, start: usize) -> JsonResult<&str> {
    if ascii_only {
        // safety: in this case we've already confirmed that all characters are ascii, we can safely
        // transmute from bytes to str
        Ok(unsafe { std::str::from_utf8_unchecked(bytes) })
    } else {
        std::str::from_utf8(bytes).map_err(|e| json_error!(InvalidUnicodeCodePoint, start + e.valid_up_to() + 1))
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
                let n2 = (((n - 0xD800) as u32) << 10 | (n2 - 0xDC00) as u32) + 0x1_0000;

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

    for c in u4.iter() {
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

pub struct StringDecoderRange;

impl<'t, 'j> AbstractStringDecoder<'t, 'j> for StringDecoderRange
where
    'j: 't,
{
    type Output = Range<usize>;

    fn decode(data: &'j [u8], mut index: usize, _tape: &'t mut Tape) -> JsonResult<(Self::Output, usize)> {
        index += 1;
        let start = index;
        while let Some(next) = data.get(index) {
            match next {
                b'"' => {
                    let r = start..index;
                    index += 1;
                    return Ok((r, index));
                }
                b'\\' => {
                    index += 1;
                    if let Some(next_inner) = data.get(index) {
                        match next_inner {
                            // these escapes are easy to validate
                            b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => (),
                            // unicode escapes are harder to validate, we just prevent them here
                            b'u' => return json_err!(StringEscapeNotSupported, index),
                            _ => return json_err!(InvalidEscape, index),
                        }
                    } else {
                        return json_err!(EofWhileParsingString, index);
                    }
                    index += 1;
                }
                _ => {
                    index += 1;
                }
            }
        }
        json_err!(EofWhileParsingString, index)
    }
}
