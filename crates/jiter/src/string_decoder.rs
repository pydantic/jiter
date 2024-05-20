use std::borrow::Cow;
use std::ops::Range;
use std::str::{from_utf8, from_utf8_unchecked};

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
    Tape(&'t str, bool),
    Data(&'j str, bool),
}

impl From<StringOutput<'_, '_>> for String {
    fn from(val: StringOutput) -> Self {
        match val {
            StringOutput::Tape(s, _) => s.to_owned(),
            StringOutput::Data(s, _) => s.to_owned(),
        }
    }
}

impl<'t, 'j> From<StringOutput<'t, 'j>> for Cow<'j, str> {
    fn from(val: StringOutput<'t, 'j>) -> Self {
        match val {
            StringOutput::Tape(s, _) => s.to_owned().into(),
            StringOutput::Data(s, _) => s.into(),
        }
    }
}

impl<'t, 'j> StringOutput<'t, 'j> {
    pub fn as_str(&self) -> &'t str {
        match self {
            Self::Tape(s, _) => s,
            Self::Data(s, _) => s,
        }
    }

    pub fn ascii_only(&self) -> bool {
        match self {
            Self::Tape(_, ascii_only) => *ascii_only,
            Self::Data(_, ascii_only) => *ascii_only,
        }
    }
}

impl<'t, 'j> AbstractStringDecoder<'t, 'j> for StringDecoder
where
    'j: 't,
{
    type Output = StringOutput<'t, 'j>;

    fn decode(data: &'j [u8], index: usize, tape: &'t mut Tape) -> JsonResult<(Self::Output, usize)> {
        let start = index + 1;

        match decode_chunk(data, start, true)? {
            (StringChunk::Quote, ascii_only, index) => {
                let s = to_str(&data[start..index], ascii_only, start)?;
                Ok((StringOutput::Data(s, ascii_only), index + 1))
            }
            (StringChunk::Backslash, ascii_only, index) => decode_to_tape(data, index, tape, start, ascii_only),
        }
    }
}

fn decode_to_tape<'t, 'j>(
    data: &'j [u8],
    mut index: usize,
    tape: &'t mut Tape,
    start: usize,
    mut ascii_only: bool,
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
                b'u' => {
                    let (c, new_index) = parse_escape(data, index)?;
                    index = new_index;
                    tape.extend_from_slice(c.encode_utf8(&mut [0_u8; 4]).as_bytes());
                }
                _ => return json_err!(InvalidEscape, index),
            }
            index += 1;
        } else {
            return json_err!(EofWhileParsingString, index);
        }

        match decode_chunk(data, index, ascii_only)? {
            (StringChunk::Quote, ascii_only, new_index) => {
                tape.extend_from_slice(&data[index..new_index]);
                index = new_index + 1;
                let s = to_str(tape, ascii_only, start)?;
                return Ok((StringOutput::Tape(s, ascii_only), index));
            }
            (StringChunk::Backslash, ascii_only_new, index_new) => {
                ascii_only = ascii_only_new;
                chunk_start = index;
                index = index_new;
            }
        }
    }
}

#[inline(always)]
pub fn decode_chunk(data: &[u8], index: usize, ascii_only: bool) -> JsonResult<(StringChunk, bool, usize)> {
    // TODO x86_64: use simd

    #[cfg(target_arch = "aarch64")]
    {
        crate::simd_aarch64::decode_string_chunk(data, index, ascii_only)
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        StringChunk::decode_fallback(data, index, ascii_only)
    }
}

pub(crate) enum StringChunk {
    Quote,
    Backslash,
}

impl StringChunk {
    #[inline(always)]
    pub fn decode_fallback(data: &[u8], mut index: usize, mut ascii_only: bool) -> JsonResult<(Self, bool, usize)> {
        while let Some(next) = data.get(index) {
            if !JSON_ASCII[*next as usize] {
                match &CHAR_TYPE[*next as usize] {
                    CharType::Quote => return Ok((Self::Quote, ascii_only, index)),
                    CharType::Backslash => return Ok((Self::Backslash, ascii_only, index)),
                    CharType::ControlChar => return json_err!(ControlCharacterWhileParsingString, index),
                    CharType::Other => {
                        ascii_only = false;
                    }
                }
            }
            index += 1;
        }
        json_err!(EofWhileParsingString, index)
    }

    /// decode an array (generally from SIMD) return the result of the chunk, or none if the non-ascii character
    /// is just > \x7F (127)
    #[inline(always)]
    #[allow(dead_code)]
    pub fn decode_array<const T: usize>(
        data: [u8; T],
        index: &mut usize,
        ascii_only: bool,
    ) -> Option<JsonResult<(Self, bool, usize)>> {
        for u8_char in data {
            if !JSON_ASCII[u8_char as usize] {
                return match &CHAR_TYPE[u8_char as usize] {
                    CharType::Quote => Some(Ok((Self::Quote, ascii_only, *index))),
                    CharType::Backslash => Some(Ok((Self::Backslash, ascii_only, *index))),
                    CharType::ControlChar => Some(json_err!(ControlCharacterWhileParsingString, *index)),
                    CharType::Other => {
                        *index += 1;
                        None
                    }
                };
            }
            *index += 1;
        }
        unreachable!("error decoding SIMD string chunk")
    }
}

// taken serde-rs/json but altered
// https://github.com/serde-rs/json/blob/ebaf61709aba7a3f2429a5d95a694514f180f565/src/read.rs#L787-L811
// this helps the fast path by telling us if something is ascii or not, it also simplifies
// CharType below by only requiring 4 options in that enum
static JSON_ASCII: [bool; 256] = {
    const CT: bool = false; // control character \x00..=\x1F
    const QU: bool = false; // quote \x22
    const BS: bool = false; // backslash \x5C
    const __: bool = true; // simple ascii
    const HI: bool = false; // > \x7F (127)
    [
        //   1   2   3   4   5   6   7   8   9   A   B   C   D   E   F
        CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, // 0
        CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, // 1
        __, __, QU, __, __, __, __, __, __, __, __, __, __, __, __, __, // 2
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 3
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 4
        __, __, __, __, __, __, __, __, __, __, __, __, BS, __, __, __, // 5
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 6
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 7
        HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, // 8
        HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, // 9
        HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, // A
        HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, // B
        HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, // C
        HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, // D
        HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, // E
        HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, HI, // F
    ]
};

enum CharType {
    // control character \x00..=\x1F
    ControlChar,
    // quote \x22
    Quote,
    // backslash \x5C
    Backslash,
    // all other characters. In reality this will only be > \x7F (127) after the JSON_ASCII check
    Other,
}

// Lookup table of bytes that must be escaped. A value of true at index i means
// that byte i requires an escape sequence in the input.
static CHAR_TYPE: [CharType; 256] = {
    const CT: CharType = CharType::ControlChar;
    const QU: CharType = CharType::Quote;
    const BS: CharType = CharType::Backslash;
    const __: CharType = CharType::Other;
    [
        //   1   2   3   4   5   6   7   8   9   A   B   C   D   E   F
        CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, // 0
        CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, // 1
        __, __, QU, __, __, __, __, __, __, __, __, __, __, __, __, __, // 2
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 3
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 4
        __, __, __, __, __, __, __, __, __, __, __, __, BS, __, __, __, // 5
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 6
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 7
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 8
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 9
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // A
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // B
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // C
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // D
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // E
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // F
    ]
};

fn to_str(bytes: &[u8], ascii_only: bool, start: usize) -> JsonResult<&str> {
    if ascii_only {
        // safety: in this case we've already confirmed that all characters are ascii, we can safely
        // transmute from bytes to str
        Ok(unsafe { from_utf8_unchecked(bytes) })
    } else {
        from_utf8(bytes).map_err(|e| json_error!(InvalidUnicodeCodePoint, start + e.valid_up_to() + 1))
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

    fn decode(data: &'j [u8], mut index: usize, _tape: &'t mut Tape) -> JsonResult<(Self::Output, usize)> {
        index += 1;
        let start = index;

        loop {
            index = match decode_chunk(data, index, true)? {
                (StringChunk::Quote, _, index) => {
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
                    b'u' => {
                        let (_, new_index) = parse_escape(data, index)?;
                        index = new_index;
                    }
                    _ => return json_err!(InvalidEscape, index),
                }
                index += 1;
            } else {
                return json_err!(EofWhileParsingString, index);
            }
        }
    }
}
