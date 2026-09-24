//! Skipping arrays and objects with 64-byte structural bitmaps.
//!
//! [`skip_container`] finds the end of the array or object at `start` without visiting each
//! value in turn. Every 64-byte block is classified into bit-per-byte masks (quotes,
//! backslashes, brackets, commas and the other structural characters, whitespace, control
//! characters); escaped quotes are removed with the backslash-parity trick, the in-string mask
//! is a prefix xor of the real quotes, and the container ends where the bracket depth returns
//! to zero. Blocks that lie inside a string need only a check for a quote, backslash or
//! control character. The grammar is checked per block from what each token follows, numbers
//! and literals from digit and dot masks or, when those do not settle it, with the decoders
//! the per-value skip uses, and escapes with its escape check, so the language accepted is
//! exactly the one `take_value_skip_per_value` accepts.
//!
//! The scan only says *whether* the container is valid. On anything else, the recursion limit
//! included, it returns `None` and the caller runs the per-value skip from the same start,
//! which reports the error where and as it always has: error parity is exact by construction
//! and error handling stays off the hot path. Strings' UTF-8 is not checked, as it never was
//! on this path.

use crate::number_decoder::{AbstractNumberDecoder, NumberRange};
use crate::parse::{FALSE_REST, NULL_REST, TRUE_REST, consume_ident};
use crate::string_decoder::skip_escape;

/// Bit-per-byte masks of one 64-byte block: bit `i` describes byte `i`.
pub(crate) struct BlockMasks {
    /// `"`
    pub(crate) quote: u64,
    /// `\`
    pub(crate) backslash: u64,
    /// `{` `}` `[` `]` `:` `,`
    pub(crate) structural: u64,
    /// `{` `}` `[` `]`
    pub(crate) brackets: u64,
    /// `,`
    pub(crate) comma: u64,
    /// space, tab, line feed, carriage return
    pub(crate) whitespace: u64,
    /// bytes below 0x20
    pub(crate) control: u64,
}

/// The masks a number token is checked against, computed only for blocks holding one.
#[derive(Clone, Copy)]
pub(crate) struct DigitMasks {
    /// `0` to `9`
    pub(crate) digit: u64,
    /// `.`
    pub(crate) dot: u64,
}

/// The index just past the array or object starting at `start`, or `None` if it is not valid
/// JSON, nests deeper than `recursion_limit`, or is unterminated.
pub(crate) fn skip_container(data: &[u8], start: usize, recursion_limit: u8, allow_inf_nan: bool) -> Option<usize> {
    debug_assert!(matches!(data.get(start), Some(b'[' | b'{')), "not at a container");
    // SAFETY: SSE2 / NEON are baseline features of the targets that compile this module.
    unsafe { skip_container_impl(data, start, recursion_limit, allow_inf_nan) }
}

/// Carries the architecture's baseline vector feature so the block classifiers inline into
/// the loop; the compiler will not inline them into a function without it.
#[cfg_attr(target_arch = "x86_64", target_feature(enable = "sse2"))]
#[cfg_attr(target_arch = "aarch64", target_feature(enable = "neon"))]
fn skip_container_impl(data: &[u8], start: usize, recursion_limit: u8, allow_inf_nan: bool) -> Option<usize> {
    let mut scanner = Scanner::new(data, start, recursion_limit, allow_inf_nan);
    let mut pos = start;
    let mut tail = [b' '; 64];
    loop {
        let (block, real) = block_at(data, pos, &mut tail);
        let step = if scanner.inside_string() && !super::block_has_string_special(block) {
            // string content only: nothing in it can end the string, and only a character
            // escaped by a backslash at the end of the previous block can need checking
            scanner.skip_string_block(pos)?;
            Step::Continue(pos + 64)
        } else {
            scanner.step(block, &super::classify_block(block), pos)?
        };
        pos = match step {
            Step::Done(end) => return Some(end),
            Step::Continue(next) => next,
        };
        if real < 64 && pos >= data.len() {
            // the input ended inside the container
            return None;
        }
    }
}

/// The block at `pos`: a view into `data`, or, past its end, `tail` holding what is left and
/// padded with spaces, which no mask reacts to. Also returns how many of the bytes were real.
#[inline(always)]
fn block_at<'a>(data: &'a [u8], pos: usize, tail: &'a mut [u8; 64]) -> (&'a [u8; 64], usize) {
    if let Some(block) = data.get(pos..pos + 64) {
        (block.try_into().unwrap(), 64)
    } else {
        let rest = &data[pos.min(data.len())..];
        *tail = [b' '; 64];
        tail[..rest.len()].copy_from_slice(rest);
        (tail, rest.len())
    }
}

const EVEN_BITS: u64 = 0x5555_5555_5555_5555;

/// The positions escaped by an odd-length run of backslashes (the character after the run),
/// with `ends_odd` carrying a run that continues across the block edge: the carry-based
/// technique of simdjson's first stage (Langdale & Lemire, "Parsing Gigabytes of JSON per
/// Second", §3.1.1).
#[inline(always)]
fn find_escaped(backslash: u64, ends_odd: &mut u64) -> u64 {
    let start_edges = backslash & !(backslash << 1);
    let even_start_mask = EVEN_BITS ^ *ends_odd;
    let even_starts = start_edges & even_start_mask;
    let odd_starts = start_edges & !even_start_mask;
    let even_carries = backslash.wrapping_add(even_starts);
    let (mut odd_carries, overflow) = backslash.overflowing_add(odd_starts);
    odd_carries |= *ends_odd;
    *ends_odd = u64::from(overflow);
    let even_carry_ends = even_carries & !backslash;
    let odd_carry_ends = odd_carries & !backslash;
    (even_carry_ends & !EVEN_BITS) | (odd_carry_ends & EVEN_BITS)
}

/// Bit `i` of the result is the xor of bits `0..=i` of `x`.
#[inline(always)]
fn prefix_xor(mut x: u64) -> u64 {
    x ^= x << 1;
    x ^= x << 2;
    x ^= x << 4;
    x ^= x << 8;
    x ^= x << 16;
    x ^= x << 32;
    x
}

/// With fewer bytes than this left in the input, the per-value skip beats the scan's fixed cost
/// per block; callers should not bother with the scan.
pub(crate) const MIN_INPUT: usize = 128;

/// Bits `from..to` set, for `from` and `to` up to 64.
#[inline(always)]
fn bits_from_to(from: usize, to: usize) -> u64 {
    let below = |n: usize| if n >= 64 { u64::MAX } else { (1u64 << n) - 1 };
    below(to) & !below(from)
}

/// The length of the literal a token starting with this byte would be, or 0.
static LITERAL_LEN: [u8; 256] = {
    let mut table = [0u8; 256];
    table[b't' as usize] = 4;
    table[b'n' as usize] = 4;
    table[b'f' as usize] = 5;
    table
};

/// A byte the `scalar` mask includes: anything but whitespace, a structural character or a
/// quote, so one that continues a number or literal token rather than ending it.
fn is_scalar_byte(byte: u8) -> bool {
    !matches!(
        byte,
        b' ' | b'\t' | b'\n' | b'\r' | b'{' | b'}' | b'[' | b']' | b':' | b',' | b'"'
    )
}

enum Step {
    /// go on with the block starting at this index
    Continue(usize),
    /// the container ended: the index just past it
    Done(usize),
}

/// What the last token of the previous block was, for the first token of the next.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Last {
    /// no token yet, or the previous block had none
    Nothing,
    OpenObject,
    OpenArray,
    Colon,
    CommaInObject,
    CommaInArray,
    Key,
    /// a scalar, a value string or a closing bracket
    Value,
}

/// What `walk_brackets` found in a block.
#[derive(Default)]
struct Brackets {
    /// bit `i` is set if byte `i` lies in an object (the parent's for an opener)
    in_object_bits: u64,
    open_object: u64,
    open_array: u64,
    close_object: u64,
    close_array: u64,
    /// the index just past the closer of the container being skipped, if it is in this block
    done: Option<usize>,
}

/// The tokens of a block, up to the container's end, by class.
struct Tokens {
    all: u64,
    open_object: u64,
    open_array: u64,
    close: u64,
    close_object: u64,
    colon: u64,
    comma_in_object: u64,
    comma_in_array: u64,
    /// opening quotes
    string: u64,
    /// the first byte of each number or literal
    scalar: u64,
    /// bit `i` is set if byte `i` lies in an object
    in_object: u64,
}

/// The state carried from one block to the next while skipping one container.
struct Scanner<'j> {
    data: &'j [u8],
    allow_inf_nan: bool,
    /// the recursion limit: how deep containers may nest
    limit: usize,
    // carried from one block to the next
    /// 1 if the previous block ended with an odd-length run of backslashes
    ends_odd_backslashes: u64,
    /// all ones if the previous block ended inside a string
    in_string: u64,
    /// 1 if the previous block ended inside a number or literal
    in_scalar: u64,
    /// escapes up to this index are checked; a `\u` surrogate pair is checked as one escape
    escapes_checked_to: usize,
    // the grammar
    last: Last,
    /// 1 inside an object, 0 inside an array
    in_object: u8,
    depth: usize,
    /// one bit per open container, from depth 1 at bit 0, set when it is an object
    objects: [u64; 4],
}

impl<'j> Scanner<'j> {
    fn new(data: &'j [u8], start: usize, recursion_limit: u8, allow_inf_nan: bool) -> Self {
        Self {
            data,
            allow_inf_nan,
            limit: usize::from(recursion_limit),
            ends_odd_backslashes: 0,
            in_string: 0,
            in_scalar: 0,
            escapes_checked_to: start,
            last: Last::Nothing,
            in_object: 0,
            depth: 0,
            objects: [0; 4],
        }
    }

    fn inside_string(&self) -> bool {
        self.in_string == u64::MAX
    }

    /// Account for the block at `pos` holding string content only: no quote, backslash or
    /// control character. Its first byte may still be escaped by a backslash run ending the
    /// previous block, in which case it must be a legal escape.
    fn skip_string_block(&mut self, pos: usize) -> Option<()> {
        debug_assert_eq!(
            self.in_scalar, 0,
            "a block ending inside a string cannot end inside a number"
        );
        if std::mem::take(&mut self.ends_odd_backslashes) != 0 && pos > self.escapes_checked_to {
            self.escapes_checked_to = skip_escape(self.data, pos).ok()?;
        }
        Some(())
    }

    /// Check the block at `pos`, whose masks are `m`, against the grammar.
    #[inline(always)]
    fn step(&mut self, block: &[u8; 64], m: &BlockMasks, pos: usize) -> Option<Step> {
        let escaped = if m.backslash == 0 {
            // only a run carried from the previous block can escape anything here, at bit 0
            std::mem::take(&mut self.ends_odd_backslashes)
        } else {
            find_escaped(m.backslash, &mut self.ends_odd_backslashes)
        };
        let quotes = m.quote & !escaped;
        // bit `i` is set if byte `i` is inside a string, its opening quote included and its
        // closing quote not
        let in_string = prefix_xor(quotes) ^ self.in_string;
        // all ones if bit 63 is set: the block ends inside a string
        self.in_string = 0u64.wrapping_sub(in_string >> 63);

        let outside = !in_string;
        let scalar = outside & !(m.structural | m.whitespace | m.quote);
        let token_starts = scalar & !((scalar << 1) | self.in_scalar);

        // A number or literal that starts in this block and runs into the next would have to
        // go to the decoders: stop this block where it starts instead and begin the next one
        // there, so it is seen whole. The byte before a token is never a backslash, a quote or
        // part of a string, so nothing else is carried across that cut.
        let (cut, next) = if scalar >> 63 != 0 && token_starts >> 1 != 0 {
            let start = 63 - token_starts.leading_zeros() as usize;
            ((1u64 << start) - 1, pos + start)
        } else {
            (u64::MAX, pos + 64)
        };
        self.in_scalar = if cut == u64::MAX { scalar >> 63 } else { 0 };
        if cut != u64::MAX {
            // the byte before the token is not a backslash, whatever the block's tail held
            self.ends_odd_backslashes = 0;
        }

        // 1. Brackets: depth, matching closers, the container's end, which bytes lie in an
        //    object, and which bracket is which.
        let Brackets {
            in_object_bits,
            open_object,
            open_array,
            close_object,
            close_array,
            done,
        } = self.walk_brackets(block, pos, m.brackets & outside & cut)?;
        // the checks over the whole block stop where the container does, or at the cut
        let region = match done {
            Some(end) if end - pos < 64 => (1u64 << (end - pos)) - 1,
            _ => cut,
        };

        // 2. The grammar, from what each token follows.
        let colon = m.structural & !m.brackets & !m.comma & outside & region;
        let comma = m.comma & outside & region;
        let string = quotes & in_string & region;
        let scalar_start = token_starts & region;
        let tokens = (m.structural & outside & region) | string | scalar_start;
        self.check_grammar(&Tokens {
            all: tokens,
            open_object,
            open_array,
            close: close_object | close_array,
            close_object,
            colon,
            comma_in_object: comma & in_object_bits,
            comma_in_array: comma & !in_object_bits,
            string,
            scalar: scalar_start,
            in_object: in_object_bits,
        })?;

        // 3. Numbers and literals.
        let mut pending = scalar_start;
        let mut digits = None;
        while pending != 0 {
            let i = pending.trailing_zeros() as usize;
            pending &= pending - 1;
            self.check_token(block, pos, i, scalar, &mut digits)?;
        }

        // 4. Strings: no control characters, and every escaped character starting a legal escape.
        if m.control & in_string & region != 0 {
            return None;
        }
        let mut to_check = escaped & in_string & region;
        while to_check != 0 {
            let at = pos + to_check.trailing_zeros() as usize;
            to_check &= to_check - 1;
            if at > self.escapes_checked_to {
                self.escapes_checked_to = skip_escape(self.data, at).ok()?;
            }
        }
        Some(match done {
            Some(end) => Step::Done(end),
            None => Step::Continue(next),
        })
    }

    /// Walk the brackets of a block (`brackets`, already outside strings and within the cut),
    /// tracking depth and container kinds. A bracket belongs to the container in effect before
    /// it: the parent's for an opener, its own for a closer. `None` on a mismatched closer or
    /// past the recursion limit.
    #[inline(always)]
    fn walk_brackets(&mut self, block: &[u8; 64], pos: usize, mut brackets: u64) -> Option<Brackets> {
        let mut out = Brackets::default();
        let mut segment_from = 0usize;
        let mut in_object = self.in_object != 0;
        while brackets != 0 {
            let i = brackets.trailing_zeros() as usize;
            let bit = brackets & brackets.wrapping_neg();
            brackets &= brackets - 1;
            if in_object {
                out.in_object_bits |= bits_from_to(segment_from, i + 1);
            }
            segment_from = i + 1;
            let byte = block[i];
            if byte == b'{' || byte == b'[' {
                if self.depth >= self.limit {
                    return None;
                }
                in_object = byte == b'{';
                if in_object {
                    out.open_object |= bit;
                } else {
                    out.open_array |= bit;
                }
                let word = &mut self.objects[self.depth / 64];
                let level_bit = 1u64 << (self.depth % 64);
                *word = if in_object {
                    *word | level_bit
                } else {
                    *word & !level_bit
                };
                self.depth += 1;
            } else {
                if in_object != (byte == b'}') {
                    return None;
                }
                if in_object {
                    out.close_object |= bit;
                } else {
                    out.close_array |= bit;
                }
                self.depth -= 1;
                if self.depth == 0 {
                    out.done = Some(pos + i + 1);
                    break;
                }
                let level = self.depth - 1;
                in_object = self.objects[level / 64] & (1u64 << (level % 64)) != 0;
            }
        }
        if in_object && out.done.is_none() {
            out.in_object_bits |= bits_from_to(segment_from, 64);
        }
        self.in_object = u8::from(in_object);
        Some(out)
    }

    /// Check what every token of a block follows. `after(x)` is the set of tokens directly
    /// following a token in `x`: subtracting the positions just past `x` from the tokens
    /// borrows up to the next token and clears it. The last token of the previous block
    /// stands in at bit 0.
    #[inline(always)]
    fn check_grammar(&mut self, t: &Tokens) -> Option<()> {
        let after = |x: u64, was_last: bool| t.all & !t.all.wrapping_sub((x << 1) | u64::from(was_last));
        let last = self.last;
        let after_open_object = after(t.open_object, last == Last::OpenObject);
        let after_comma_in_object = after(t.comma_in_object, last == Last::CommaInObject);
        let key = t.string & (after_open_object | after_comma_in_object);
        let value = (t.string & !key) | t.scalar | t.close;
        let invalid =
            // after `{`: a key or `}`
            (after_open_object & !(t.string | t.close_object))
            // after `[`: a value or `]`
            | (after(t.open_array, last == Last::OpenArray) & (t.colon | t.comma_in_array | t.close_object))
            // after `:`: a value
            | (after(t.colon, last == Last::Colon) & (t.colon | t.comma_in_object | t.close))
            // after `,` in an object: a key
            | (after_comma_in_object & !t.string)
            // after `,` in an array: a value
            | (after(t.comma_in_array, last == Last::CommaInArray) & (t.colon | t.comma_in_array | t.close))
            // after a key: `:`
            | (after(key, last == Last::Key) & !t.colon)
            // after a value: `,` or the end of its container
            | (after(value, last == Last::Value) & !(t.comma_in_object | t.comma_in_array | t.close))
            // colons belong in objects
            | (t.colon & !t.in_object);
        if invalid != 0 {
            return None;
        }
        if t.all != 0 {
            let last_bit = 1u64 << t.all.ilog2();
            self.last = if last_bit & t.open_object != 0 {
                Last::OpenObject
            } else if last_bit & t.open_array != 0 {
                Last::OpenArray
            } else if last_bit & t.colon != 0 {
                Last::Colon
            } else if last_bit & t.comma_in_object != 0 {
                Last::CommaInObject
            } else if last_bit & t.comma_in_array != 0 {
                Last::CommaInArray
            } else if last_bit & key != 0 {
                Last::Key
            } else {
                Last::Value
            };
        }
        Some(())
    }

    /// The token starting at bit `i` of the block at `pos`. A literal is compared as a word; a
    /// number, `-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?`, is checked from the digit and
    /// dot masks; anything else, and any token reaching the block's end, goes to the decoders.
    #[inline(always)]
    fn check_token(
        &mut self,
        block: &[u8; 64],
        pos: usize,
        i: usize,
        scalar: u64,
        digits: &mut Option<DigitMasks>,
    ) -> Option<()> {
        let at = pos + i;
        let first = block[i];
        let bit = 1u64 << i;
        // the run of scalar bytes from `i`: adding `bit` carries through it and stops just past it
        let token = scalar & (scalar ^ scalar.wrapping_add(bit));
        if token >> 63 != 0 {
            // reaches the block's end
            return self.check_scalar(first, at);
        }
        let literal_len = usize::from(LITERAL_LEN[usize::from(first)]);
        if literal_len != 0 {
            // the whole token must be the literal: exactly its length, and its bytes; compared
            // as a mask because the x86_64 baseline has no popcnt and its expansion is costly
            if token != ((1u64 << literal_len) - 1) << i {
                return None;
            }
            let word = u32::from_le_bytes(block[i..i + 4].try_into().unwrap());
            let matches = match first {
                b't' => word == u32::from_le_bytes(*b"true"),
                b'n' => word == u32::from_le_bytes(*b"null"),
                _ => word == u32::from_le_bytes(*b"fals") && block[i + 4] == b'e',
            };
            return matches.then_some(());
        }
        let DigitMasks { digit, dot } = *digits.get_or_insert_with(|| super::digit_masks(block));
        let negative = first == b'-';
        let body = if negative { token & !bit } else { token };
        // the mantissa: digits and dots up to the first byte that is neither
        let other = body & !(digit | dot);
        let mantissa = if other == 0 {
            body
        } else {
            body & ((other & other.wrapping_neg()) - 1)
        };
        if mantissa == 0 {
            // `-` alone, or something that is not a number, such as `NaN`: the decoder decides
            return self.check_scalar(first, at);
        }
        let dots = dot & mantissa;
        // at most one dot, with a digit on both sides: neither the first byte of the mantissa
        // (its predecessor is outside it) nor the last (its successor is)
        if dots & dots.wrapping_sub(1) != 0 || dots & !((mantissa << 1) & (mantissa >> 1)) != 0 {
            return None;
        }
        // a leading zero stands alone: no digit may follow it
        let first_digit = i + usize::from(negative);
        if block[first_digit] == b'0' && (digit >> (first_digit + 1)) & 1 != 0 {
            return None;
        }
        // an exponent: `e` or `E`, an optional sign, then digits to the end of the token
        let mut exponent = body & !mantissa;
        if exponent != 0 {
            let e = exponent & exponent.wrapping_neg();
            if !matches!(block[e.trailing_zeros() as usize], b'e' | b'E') {
                return self.check_scalar(first, at);
            }
            exponent &= !e;
            let sign = exponent & exponent.wrapping_neg();
            if sign != 0 && matches!(block[sign.trailing_zeros() as usize], b'+' | b'-') {
                exponent &= !sign;
            }
            if exponent == 0 || exponent & !digit != 0 {
                return None;
            }
        }
        Some(())
    }

    /// A number or literal starts at `at`: check it with the decoders the per-value skip uses.
    fn check_scalar(&mut self, first: u8, at: usize) -> Option<()> {
        let end = match first {
            b't' => consume_ident(self.data, at, TRUE_REST),
            b'f' => consume_ident(self.data, at, FALSE_REST),
            b'n' => consume_ident(self.data, at, NULL_REST),
            _ => NumberRange::decode(self.data, at, first, self.allow_inf_nan).map(|(_, end)| end),
        }
        .ok()?;
        // the token must end where the number or literal does
        if self.data.get(end).is_some_and(|&next| is_scalar_byte(next)) {
            return None;
        }
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::errors::{DEFAULT_RECURSION_LIMIT, JsonErrorType};
    use crate::parse::Parser;
    use crate::string_decoder::Tape;
    use crate::value::take_value_skip_per_value;

    /// The masks a block should produce, byte by byte: quote, backslash, structural, brackets,
    /// comma, whitespace, control, digit, dot.
    fn reference_masks(block: &[u8; 64]) -> [u64; 9] {
        let mut m = [0u64; 9];
        for (i, &b) in block.iter().enumerate() {
            let bit = 1u64 << i;
            match b {
                b'"' => m[0] |= bit,
                b'\\' => m[1] |= bit,
                b'{' | b'}' | b'[' | b']' => {
                    m[2] |= bit;
                    m[3] |= bit;
                }
                b',' => {
                    m[2] |= bit;
                    m[4] |= bit;
                }
                b':' => m[2] |= bit,
                b' ' | b'\t' | b'\n' | b'\r' => m[5] |= bit,
                b'0'..=b'9' => m[7] |= bit,
                b'.' => m[8] |= bit,
                _ => {}
            }
            if b < 0x20 {
                m[6] |= bit;
            }
        }
        m
    }

    fn interesting_byte() -> impl Strategy<Value = u8> {
        prop_oneof![
            6 => prop::sample::select(b"\"\\{}[]:, \t\n\r.-0129/e".to_vec()),
            2 => prop::sample::select(vec![0x00u8, 0x01, 0x1f, 0x20, 0x21, 0x7f, 0x80, 0xc3, 0xff]),
            6 => b'a'..=b'z',
            2 => b'0'..=b'9',
            1 => any::<u8>(),
        ]
    }

    /// Escaped positions and in-string bits, from the definition: a character is escaped when
    /// an odd number of backslashes immediately precedes it (a backslash extends the run
    /// instead), an unescaped quote toggles being in a string, and a string's opening quote
    /// counts as inside it.
    fn reference_string_state(data: &[u8]) -> (Vec<bool>, Vec<bool>) {
        let mut escaped = Vec::with_capacity(data.len());
        let mut inside = Vec::with_capacity(data.len());
        let mut run = 0usize;
        let mut in_string = false;
        for &b in data {
            let is_escaped = b != b'\\' && run % 2 == 1;
            escaped.push(is_escaped);
            if b == b'\\' {
                run += 1;
            } else {
                if b == b'"' && !is_escaped {
                    in_string = !in_string;
                }
                run = 0;
            }
            inside.push(in_string);
        }
        (escaped, inside)
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(2000))]

        #[test]
        fn classify_block_matches_reference(bytes in prop::collection::vec(interesting_byte(), 64)) {
            let block: &[u8; 64] = bytes.as_slice().try_into().unwrap();
            let m = crate::simd::classify_block(block);
            let d = crate::simd::digit_masks(block);
            prop_assert_eq!(
                [m.quote, m.backslash, m.structural, m.brackets, m.comma, m.whitespace, m.control, d.digit, d.dot],
                reference_masks(block)
            );
            prop_assert_eq!(
                crate::simd::block_has_string_special(block),
                m.quote | m.backslash | m.control != 0
            );
        }

        /// The carried escape and string state must match the definition across block edges.
        #[test]
        fn string_state_matches_reference(data in prop::collection::vec(interesting_byte(), 0..300)) {
            let (want_escaped, want_inside) = reference_string_state(&data);
            let mut ends_odd = 0u64;
            let mut carried_in_string = 0u64;
            let mut tail = [b' '; 64];
            let mut pos = 0;
            while pos < data.len() {
                let (block, real) = block_at(&data, pos, &mut tail);
                let m = crate::simd::classify_block(block);
                let escaped = find_escaped(m.backslash, &mut ends_odd);
                let in_string = prefix_xor(m.quote & !escaped) ^ carried_in_string;
                carried_in_string = 0u64.wrapping_sub(in_string >> 63);
                for i in 0..real {
                    prop_assert_eq!(escaped & (1u64 << i) != 0, want_escaped[pos + i], "escaped bit at {}", pos + i);
                    prop_assert_eq!(in_string & (1u64 << i) != 0, want_inside[pos + i], "in-string bit at {}", pos + i);
                }
                pos += 64;
            }
        }
    }

    fn json_string() -> impl Strategy<Value = String> {
        prop_oneof![
            4 => prop::collection::vec(
                prop_oneof![
                    8 => any::<char>(),
                    1 => Just('"'), 1 => Just('\\'), 1 => Just('/'), 1 => Just('\n'), 1 => Just('\t'),
                    1 => Just('{'), 1 => Just('}'), 1 => Just('['), 1 => Just(']'), 1 => Just(','),
                    1 => Just('\u{1}'), 1 => Just('\u{1F600}'), 1 => Just('\u{e9}'),
                ],
                0..40,
            )
            .prop_map(|chars| serde_json::to_string(&chars.into_iter().collect::<String>()).unwrap()),
            // long enough to span several blocks
            1 => (60usize..300).prop_map(|n| format!("\"{}\"", "abcdefgh".repeat(n / 8 + 1))),
            // explicit escapes, surrogate pairs included
            1 => prop::sample::select(vec![
                r#""éA""#, r#""😀""#, r#""a\\\"b""#, r#""\/\b\f\n\r\t""#,
                r#""é tail""#, "\"\\\\\\\\\"",
            ]).prop_map(str::to_string),
        ]
    }

    fn json_number() -> impl Strategy<Value = String> {
        prop_oneof![
            any::<i64>().prop_map(|i| i.to_string()),
            "-?(0|[1-9][0-9]{0,25})(\\.[0-9]{1,30})?([eE][+-]?[0-9]{1,3})?",
            any::<f64>()
                .prop_filter("finite", |f| f.is_finite())
                .prop_map(|f| format!("{f:?}")),
            prop::sample::select(vec!["NaN", "Infinity", "-Infinity"]).prop_map(str::to_string),
        ]
    }

    /// Well-formed documents (bar the `NaN`/`Infinity` extension, which only `allow_inf_nan`
    /// admits), with varied whitespace, as text so number spellings are generated too.
    fn json_text() -> impl Strategy<Value = String> {
        let ws = prop::sample::select(vec!["", " ", "\n", "\t ", " \r\n  "]);
        let leaf = prop_oneof![
            Just("null".to_string()),
            Just("true".to_string()),
            Just("false".to_string()),
            json_number(),
            json_string(),
        ];
        leaf.prop_recursive(6, 80, 8, move |inner| {
            let ws2 = ws.clone();
            prop_oneof![
                (prop::collection::vec(inner.clone(), 0..8), ws.clone())
                    .prop_map(|(items, w)| { format!("[{w}{}{w}]", items.join(&format!("{w},{w}"))) }),
                (prop::collection::vec((json_string(), inner), 0..8), ws2).prop_map(|(entries, w)| {
                    let body = entries
                        .iter()
                        .map(|(k, v)| format!("{k}{w}:{w}{v}"))
                        .collect::<Vec<_>>()
                        .join(&format!("{w},{w}"));
                    format!("{{{w}{body}{w}}}")
                }),
            ]
        })
    }

    /// A single edit that usually breaks a document.
    #[derive(Debug, Clone)]
    enum Edit {
        Replace(prop::sample::Index, u8),
        Insert(prop::sample::Index, u8),
        Delete(prop::sample::Index),
        Truncate(prop::sample::Index),
    }

    fn edit() -> impl Strategy<Value = Edit> {
        let byte = prop::sample::select(b"\"\\{}[]:, x0-.e\x01\xc3u".to_vec());
        prop_oneof![
            (any::<prop::sample::Index>(), byte.clone()).prop_map(|(i, b)| Edit::Replace(i, b)),
            (any::<prop::sample::Index>(), byte).prop_map(|(i, b)| Edit::Insert(i, b)),
            any::<prop::sample::Index>().prop_map(Edit::Delete),
            any::<prop::sample::Index>().prop_map(Edit::Truncate),
        ]
    }

    fn apply(data: &mut Vec<u8>, edit: &Edit) {
        if data.is_empty() {
            return;
        }
        match *edit {
            Edit::Replace(i, b) => {
                let at = i.index(data.len());
                data[at] = b;
            }
            Edit::Insert(i, b) => data.insert(i.index(data.len() + 1), b),
            Edit::Delete(i) => {
                data.remove(i.index(data.len()));
            }
            Edit::Truncate(i) => data.truncate(i.index(data.len() + 1)),
        }
    }

    type SkipResult = Result<usize, (JsonErrorType, usize)>;

    /// The per-value skip's verdict on the value at the start of `data`.
    fn per_value_skip(data: &[u8], allow_inf_nan: bool) -> SkipResult {
        let mut parser = Parser::new(data);
        let mut tape = Tape::default();
        parser
            .peek()
            .and_then(|peek| {
                take_value_skip_per_value(peek, &mut parser, &mut tape, DEFAULT_RECURSION_LIMIT, allow_inf_nan)
            })
            .map(|()| parser.index)
            .map_err(|e| (e.error_type, e.index))
    }

    /// Both skips must agree on the container at the start of `data`: the same end when it
    /// is valid, a rejection from the scan whenever the per-value skip errors.
    fn assert_agree(data: &[u8], allow_inf_nan: bool) -> Result<(), TestCaseError> {
        if !matches!(data.first(), Some(b'[' | b'{')) {
            return Ok(());
        }
        let want = per_value_skip(data, allow_inf_nan);
        let got = skip_container(data, 0, DEFAULT_RECURSION_LIMIT, allow_inf_nan);
        prop_assert_eq!(
            got,
            want.clone().ok(),
            "allow_inf_nan={} on {:?}: per-value skip {:?}",
            allow_inf_nan,
            String::from_utf8_lossy(data),
            want
        );
        Ok(())
    }

    proptest! {
        // 20k cases of each ran on both architectures while this was written; enough here to
        // catch a regression without slowing the suite
        #![proptest_config(ProptestConfig::with_cases(1000))]

        #[test]
        fn agrees_with_per_value_skip_on_valid_documents(json in json_text()) {
            assert_agree(json.as_bytes(), false)?;
            assert_agree(json.as_bytes(), true)?;
        }

        #[test]
        fn agrees_with_per_value_skip_on_edited_documents(json in json_text(), edits in prop::collection::vec(edit(), 1..3)) {
            let mut data = json.into_bytes();
            for edit in &edits {
                apply(&mut data, edit);
            }
            assert_agree(&data, false)?;
            assert_agree(&data, true)?;
        }
    }

    /// Hand-picked shapes: empty and whitespace-only containers, escapes of every kind and
    /// escapes and tokens straddling a 64-byte block edge, every grammar error the per-value
    /// skip reports, the recursion limit on both sides, and what follows a container's end.
    fn edge_cases() -> Vec<String> {
        let deep = |n: usize| format!("{}1{}", "[".repeat(n), "]".repeat(n));
        vec![
            "[]".into(),
            "{}".into(),
            "[ ]".into(),
            "{ }".into(),
            r#"{"a":"}"}"#.into(),
            r#"["\"]", 1]"#.into(),
            r#"["\\"]"#.into(),
            r#"["\\\"]"]"#.into(),
            r#"["😀"]"#.into(),
            r#"["\ud83d"]"#.into(),
            r#"["\ude00"]"#.into(),
            r#"["\ud83dA"]"#.into(),
            r#"["\u12"]"#.into(),
            r#"["\x"]"#.into(),
            r#"["\"#.into(),
            "[\"\u{1}\"]".into(),
            "[\"\t\"]".into(),
            format!("[{}]", "\"x\",".repeat(30) + "1"),
            format!("{{\"k\":\"{}\"}}", "\\\\".repeat(70)),
            format!("{{\"k\":\"{}\\\"\"}}", "\\\\".repeat(31)),
            format!("[\"{}\"]", "a".repeat(200)),
            format!("[\"{}\\n\"]", "a".repeat(63)),
            format!("[\"{}\\u00e9\"]", "a".repeat(61)),
            format!("[{}1]", " ".repeat(63)),
            format!("[{}]", "1,".repeat(40) + "1"),
            format!("[{}12345]", " ".repeat(60)),
            format!("[{}true]", " ".repeat(61)),
            format!("[{}truex]", " ".repeat(61)),
            "[1,2".into(),
            r#"{"a":"unterminated}"#.into(),
            "[1,]".into(),
            r#"{"a":1,}"#.into(),
            "[,1]".into(),
            "[1 2]".into(),
            r#"{"a" 1}"#.into(),
            r#"{"a":}"#.into(),
            r#"{"a"}"#.into(),
            "{,}".into(),
            "{1:2}".into(),
            "[1}".into(),
            "{]".into(),
            r#"{"a":[1}]"#.into(),
            "[01]".into(),
            "[1.]".into(),
            "[1e]".into(),
            "[-]".into(),
            "[+1]".into(),
            "[.5]".into(),
            "[1.5e3]".into(),
            "[123abc]".into(),
            "[nul]".into(),
            "[nulll]".into(),
            "[NaN]".into(),
            "[Infinity]".into(),
            "[-Infinity]".into(),
            "[-Infinityx]".into(),
            "[Inf]".into(),
            "[\u{e9}]".into(),
            "[\u{1}]".into(),
            "[\"a\"\"b\"]".into(),
            "[1\"a\"]".into(),
            deep(199),
            deep(200),
            deep(201),
            deep(1000),
            // the container ends and what follows must not be looked at, even in the same block
            "[]\n\"\u{1}\"".into(),
            "[]\"\\x\"".into(),
            "{}}".into(),
            "[]]".into(),
            "[] x".into(),
            "{}\"\\ud83d\"".into(),
            "[1]\"unterminated".into(),
            "[[]]\"\u{1}".into(),
            format!("[]{}\"\u{1}\"", " ".repeat(70)),
            format!("[\"{}\"]\u{1}", "a".repeat(60)),
        ]
    }

    /// Shapes that straddle a 64-byte block edge or sit at the end of a block: escapes, long
    /// tokens, and literals whose token is shorter than the literal.
    fn block_edge_cases() -> Vec<String> {
        vec![
            // an escape straddling a block edge, the rest of the next block plain string content
            format!("[\"{}\\n{}\"]", "a".repeat(61), "b".repeat(70)),
            format!("[\"{}\\a{}\"]", "a".repeat(61), "b".repeat(70)),
            format!("[\"{}\\u00e9{}\"]", "a".repeat(61), "b".repeat(70)),
            format!("[\"{}\\u00g9{}\"]", "a".repeat(61), "b".repeat(70)),
            format!("[\"{}\\\"{}\"]", "a".repeat(61), "b".repeat(70)),
            format!("[\"{}\\\\{}\"]", "a".repeat(61), "b".repeat(70)),
            format!("[\"{}\\\\{}\"]", "a".repeat(62), "b".repeat(70)),
            format!("[\"{}\\ud83d\\ude00{}\"]", "a".repeat(58), "b".repeat(70)),
            format!("[\"{}\\ud83d\\ude00{}\"]", "a".repeat(55), "b".repeat(70)),
            format!("{}1{}", "{\"a\":".repeat(200), "}".repeat(200)),
            format!("{}1{}", "{\"a\":".repeat(201), "}".repeat(201)),
            // literals entirely inside a block, ending at its last byte or just before
            format!("[{}true]", " ".repeat(58)),
            format!("[{}null]", " ".repeat(58)),
            format!("[{}false]", " ".repeat(57)),
            format!("[{}true,1]", " ".repeat(30)),
            format!("[{}false,null,true]", " ".repeat(20)),
            format!("[{}truex]", " ".repeat(30)),
            // right length, wrong bytes: only the word compare can reject these
            format!("[{}trux]", " ".repeat(58)),
            format!("[{}nulx]", " ".repeat(30)),
            format!("[{}fxlse]", " ".repeat(30)),
            format!("[{}falsx]", " ".repeat(57)),
            format!("[{}fals]", " ".repeat(30)),
            format!("[{}nulll]", " ".repeat(30)),
            format!("[{}tru]", " ".repeat(58)),
            format!("[{}nul]", " ".repeat(30)),
            // a literal's first byte near the end of a block, the token shorter than the literal
            format!("[{}t,1]", " ".repeat(60)),
            format!("[{}fa,1]", " ".repeat(59)),
            format!("[{}nul]", " ".repeat(60)),
            format!("[{}true]", " ".repeat(59)),
            format!("[{}-5]", " ".repeat(60)),
            // tokens of a block or more, starting like a literal, a number, or nothing known
            format!("[{}]", "t".repeat(70)),
            format!("[{}]", "false".repeat(20)),
            format!("[{}]", "n".repeat(64)),
            format!("[{}]", "1".repeat(70)),
            format!("[{}]", "x".repeat(70)),
            format!("[{}]", "-".repeat(70)),
            format!("[{}.{}e{}]", "1".repeat(30), "2".repeat(30), "3".repeat(30)),
        ]
    }

    #[test]
    fn edge_cases_agree_with_per_value_skip() {
        for case in edge_cases().iter().chain(&block_edge_cases()) {
            let data = case.as_bytes();
            for allow_inf_nan in [false, true] {
                let want = per_value_skip(data, allow_inf_nan);
                let got = skip_container(data, 0, DEFAULT_RECURSION_LIMIT, allow_inf_nan);
                assert_eq!(
                    got,
                    want.clone().ok(),
                    "allow_inf_nan={allow_inf_nan} on {case:?}: per-value skip {want:?}"
                );
            }
        }
    }

    /// The scan honours a smaller recursion budget, as serde's ignored-any passes it one.
    #[test]
    fn honours_the_recursion_limit_given() {
        let nested = |n: usize| format!("{}1{}", "[".repeat(n), "]".repeat(n));
        for limit in [1u8, 2, 3, 10] {
            for n in 1..=12 {
                let data = nested(n);
                let got = skip_container(data.as_bytes(), 0, limit, false);
                assert_eq!(got.is_some(), n <= usize::from(limit), "limit {limit}, {n} deep");
            }
        }
    }
}
