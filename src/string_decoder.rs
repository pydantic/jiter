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
    Tape(&'t str),
    Data(&'j str),
}

impl From<StringOutput<'_, '_>> for String {
    fn from(val: StringOutput) -> Self {
        match val {
            StringOutput::Tape(s) => s.to_owned(),
            StringOutput::Data(s) => s.to_owned(),
        }
    }
}

impl<'t, 'j> From<StringOutput<'t, 'j>> for Cow<'j, str> {
    fn from(val: StringOutput<'t, 'j>) -> Self {
        match val {
            StringOutput::Tape(s) => s.to_owned().into(),
            StringOutput::Data(s) => s.into(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StrType<'a> {
    pub s: &'a str,
    pub known_ascii: bool,
}

impl<'t, 'j> StringOutput<'t, 'j> {
    pub fn as_str(&self) -> &'t str {
        match self {
            Self::Tape(s) => s,
            Self::Data(s) => s,
        }
    }

    pub fn as_str_type(&self) -> StrType<'t> {
        match self {
            Self::Tape(s) => StrType { s, known_ascii: false },
            Self::Data(s) => StrType { s, known_ascii: true },
        }
    }
}

// taken serde-rs/json but altered
// https://github.com/serde-rs/json/blob/ebaf61709aba7a3f2429a5d95a694514f180f565/src/read.rs#L787-L811
// this helps the fast path by telling us if something is ascii or not, it also simplifies
// CharType below by only requiring 4 options in that enum
static ASCII: [bool; 256] = {
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
    // all other characters. In reality this will only be > \x7F (127) after the ASCII check
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

impl<'t, 'j> AbstractStringDecoder<'t, 'j> for StringDecoder
where
    'j: 't,
{
    type Output = StringOutput<'t, 'j>;

    fn decode(data: &'j [u8], mut index: usize, tape: &'t mut Tape) -> JsonResult<(Self::Output, usize)> {
        index += 1;
        let start = index;
        let mut ascii_only = true;

        while let Some(next) = data.get(index) {
            if !ASCII[*next as usize] {
                match &CHAR_TYPE[*next as usize] {
                    CharType::Quote => {
                        let s = to_str(&data[start..index], ascii_only, start)?;
                        index += 1;
                        return Ok((StringOutput::Data(s), index));
                    }
                    CharType::Backslash => return decode_to_tape(data, index, tape, start, ascii_only),
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
}

fn decode_to_tape<'t, 'j>(
    data: &'j [u8],
    mut index: usize,
    tape: &'t mut Tape,
    start: usize,
    mut ascii_only: bool,
) -> JsonResult<(StringOutput<'t, 'j>, usize)> {
    let mut last_escape = start;
    tape.clear();

    macro_rules! on_backslash {
        () => {{
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
                return json_err!(EofWhileParsingString, index);
            }
        }};
    }

    on_backslash!();
    index += 1;

    while let Some(next) = data.get(index) {
        if !ASCII[*next as usize] {
            match &CHAR_TYPE[*next as usize] {
                CharType::Quote => {
                    tape.extend_from_slice(&data[last_escape..index]);
                    index += 1;
                    let s = to_str(tape, ascii_only, start)?;
                    return Ok((StringOutput::Tape(s), index));
                }
                CharType::Backslash => on_backslash!(),
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
