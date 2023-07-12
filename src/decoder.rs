use std::ops::Range;
use crate::{JsonError, JsonResult};

pub trait DecodeString {
    type Output;

    fn decode(data: &[u8], index: usize) -> JsonResult<(Self::Output, usize)>;
}

pub struct DecodeStringString;

impl DecodeString for DecodeStringString {
    type Output = String;

    fn decode(data: &[u8], mut index: usize) -> JsonResult<(Self::Output, usize)> {
        index += 1;
        let mut chars = Vec::new();
        while let Some(next) = data.get(index) {
            match next {
                b'"' => {
                    index += 1;
                    let s = unsafe { String::from_utf8_unchecked(chars) };
                    return Ok((s, index));
                }
                b'\\' => {
                    index += 1;
                    if let Some(next_inner) = data.get(index) {
                        match next_inner {
                            b'"' | b'\\' | b'/' => chars.push(*next_inner),
                            b'b' => chars.push(b'\x08'),
                            b'f' => chars.push(b'\x0C'),
                            b'n' => chars.push(b'\n'),
                            b'r' => chars.push(b'\r'),
                            b't' => chars.push(b'\t'),
                            b'u' => {
                                let mut n = 0;
                                for _ in 0..4 {
                                    index += 1;
                                    let c = match data.get(index) {
                                        Some(c) => *c,
                                        None => return Err(JsonError::InvalidString(index)),
                                    };
                                    let hex = match c {
                                        b'0'..=b'9' => (c & 0x0f) as u16,
                                        b'a'..=b'f' => (c - b'a' + 10) as u16,
                                        b'A'..=b'F' => (c - b'A' + 10) as u16,
                                        _ => return Err(JsonError::InvalidStringEscapeSequence(index)),
                                    };
                                    n = (n << 4) + hex;
                                }
                                match char::from_u32(n as u32) {
                                    Some(c) => {
                                        for b in c.to_string().bytes() {
                                            chars.push(b);
                                        }
                                    }
                                    None => return Err(JsonError::InvalidString(index)),
                                }
                            },
                            _ => return Err(JsonError::InvalidString(index)),
                        }
                    } else {
                        return Err(JsonError::UnexpectedEnd);
                    }
                }
                // 8 = backspace, 9 = tab, 10 = newline, 12 = formfeed, 13 = carriage return
                8 | 9 | 10 | 12 | 13 => return Err(JsonError::InvalidString(index)),
                _ => chars.push(*next),
            }
            index += 1;
        }
        Err(JsonError::UnexpectedEnd)
    }
}

// should be changed to bytes
pub struct DecodeStringRange;

impl DecodeString for DecodeStringRange {
    type Output = Range<usize>;

    fn decode(data: &[u8], mut index: usize) -> JsonResult<(Self::Output, usize)> {
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
                    index += 2;
                    // TODO check hex escape sequence
                }
                _ => {
                    index += 1;
                }
            }
        }
        Err(JsonError::UnexpectedEnd)
    }
}

