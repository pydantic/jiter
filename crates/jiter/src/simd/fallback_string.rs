use crate::string_decoder::StringChunk;

use crate::errors::{JsonResult, json_err};

pub(crate) fn decode_string_chunk(
    data: &[u8],
    mut index: usize,
    mut ascii_only: bool,
    allow_partial: bool,
) -> JsonResult<(StringChunk, bool, usize)> {
    while let Some(next) = data.get(index) {
        if !JSON_ASCII[*next as usize] {
            match &CHAR_TYPE[*next as usize] {
                CharType::Quote => return Ok((StringChunk::StringEnd, ascii_only, index)),
                CharType::Backslash => return Ok((StringChunk::Backslash, ascii_only, index)),
                CharType::ControlChar => return json_err!(ControlCharacterWhileParsingString, index),
                CharType::Other => {
                    ascii_only = false;
                }
            }
        }
        index += 1;
    }
    if allow_partial {
        Ok((StringChunk::StringEnd, ascii_only, index))
    } else {
        json_err!(EofWhileParsingString, index)
    }
}

// taken serde-rs/json but altered
// https://github.com/serde-rs/json/blob/ebaf61709aba7a3f2429a5d95a694514f180f565/src/read.rs#L787-L811
// this helps the fast path by telling us if something is ascii or not, it also simplifies
// CharType below by only requiring 4 options in that enum
pub(crate) static JSON_ASCII: [bool; 256] = {
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

pub(crate) enum CharType {
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
pub(crate) static CHAR_TYPE: [CharType; 256] = {
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
