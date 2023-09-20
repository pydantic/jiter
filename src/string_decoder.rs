use std::marker::PhantomData;
use std::ops::Range;

use crate::errors::{json_err, json_error, JsonResult};

pub type Tape = Vec<u8>;

pub trait AbstractStringDecoder<'t> {
    type Output;

    fn decode<'d>(data: &'d [u8], index: usize, tape: &'t mut Tape) -> JsonResult<(Self::Output, usize)>
    where
        'd: 't;
}

pub struct StringDecoder<'t> {
    _phantom: &'t PhantomData<()>,
}

impl<'t> AbstractStringDecoder<'t> for StringDecoder<'t> {
    type Output = &'t str;

    fn decode<'d>(data: &'d [u8], mut index: usize, tape: &'t mut Tape) -> JsonResult<(Self::Output, usize)>
    where
        'd: 't,
    {
        index += 1;
        tape.clear();
        let start = index;
        let mut last_escape = start;
        let mut found_escape = false;
        let mut ascii_only = true;

        while let Some(next) = data.get(index) {
            match next {
                b'"' => {
                    // in theory we could use `std::str::from_utf8_unchecked` here,
                    // it leads to big performance gains but some cases e.g. good_high_order_string
                    // passing when they error here, serde uses `std::str::from_utf8`, python's `json.loads`
                    // allows these higher order strings
                    let s = if found_escape {
                        tape.extend_from_slice(&data[last_escape..index]);
                        to_str(tape, ascii_only, start)?
                    } else {
                        to_str(&data[start..index], ascii_only, start)?
                    };
                    index += 1;
                    return Ok((s, index));
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
                                let (c, new_index) = parse_escape(data, index, start)?;
                                index = new_index;
                                tape.extend_from_slice(c.encode_utf8(&mut [0_u8; 4]).as_bytes());
                            }
                            _ => return json_err!(InvalidString, index - start, start - 1),
                        }
                        last_escape = index + 1;
                    } else {
                        return json_err!(UnexpectedEnd, start - 1);
                    }
                }
                // all values below 32 are invalid
                next if *next < 32u8 => return json_err!(InvalidString, index - start, start - 1),
                next if *next >= 128u8 && ascii_only => {
                    ascii_only = false;
                }
                _ => (),
            }
            index += 1;
        }
        json_err!(UnexpectedEnd, index)
    }
}

fn to_str(bytes: &[u8], ascii_only: bool, start: usize) -> JsonResult<&str> {
    if ascii_only {
        // safety: in this case we've already confirmed that all characters are ascii, we can safely
        // transmute from bytes to str
        Ok(unsafe { std::str::from_utf8_unchecked(bytes) })
    } else {
        std::str::from_utf8(bytes).map_err(|e| json_error!(InvalidString, e.valid_up_to(), start - 1))
    }
}

/// Taken from https://github.com/serde-rs/json/blob/45f10ec816e3f2765ac08f7ca73752326b0475d7/src/read.rs#L873-L928
fn parse_escape(data: &[u8], index: usize, start: usize) -> JsonResult<(char, usize)> {
    let (n, index) = parse_u4(data, index, start)?;
    match n {
        0xDC00..=0xDFFF => json_err!(InvalidStringEscapeSequence, index - start, start - 1),
        0xD800..=0xDBFF => match (data.get(index + 1), data.get(index + 2)) {
            (Some(b'\\'), Some(b'u')) => {
                let (n2, index) = parse_u4(data, index + 2, start)?;
                if !(0xDC00..=0xDFFF).contains(&n2) {
                    return json_err!(InvalidStringEscapeSequence, index - start, start - 1);
                }
                let n2 = (((n - 0xD800) as u32) << 10 | (n2 - 0xDC00) as u32) + 0x1_0000;

                match char::from_u32(n2) {
                    Some(c) => Ok((c, index)),
                    None => json_err!(InvalidString, index - start, start - 1),
                }
            }
            _ => json_err!(InvalidStringEscapeSequence, index - start, start - 1),
        },
        _ => match char::from_u32(n as u32) {
            Some(c) => Ok((c, index)),
            None => json_err!(InvalidString, index - start, start - 1),
        },
    }
}

fn parse_u4(data: &[u8], mut index: usize, start: usize) -> JsonResult<(u16, usize)> {
    let mut n = 0;
    for _ in 0..4 {
        index += 1;
        let c = match data.get(index) {
            Some(c) => *c,
            None => return json_err!(InvalidString, index - start, start - 1),
        };
        let hex = match c {
            b'0'..=b'9' => (c & 0x0f) as u16,
            b'a'..=b'f' => (c - b'a' + 10) as u16,
            b'A'..=b'F' => (c - b'A' + 10) as u16,
            _ => return json_err!(InvalidStringEscapeSequence, index - start, start - 1),
        };
        n = (n << 4) + hex;
    }
    Ok((n, index))
}

pub struct StringDecoderRange;

impl<'t> AbstractStringDecoder<'t> for StringDecoderRange {
    type Output = Range<usize>;

    fn decode<'d>(data: &'d [u8], mut index: usize, _tape: &'t mut Tape) -> JsonResult<(Self::Output, usize)>
    where
        'd: 't,
    {
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
                            b'u' => return json_err!(StringEscapeNotSupported, index - start, start - 1),
                            _ => return json_err!(InvalidString, index - start, start - 1),
                        }
                    } else {
                        return json_err!(UnexpectedEnd, start - 1);
                    }
                    index += 1;
                }
                _ => {
                    index += 1;
                }
            }
        }
        json_err!(UnexpectedEnd, start - 1)
    }
}
