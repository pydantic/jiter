use std::cmp::Ordering;
use std::fmt;
use std::mem::size_of;
use std::num::TryFromIntError;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use jiter::{JsonObject, JsonValue};

use crate::decoder::Decoder;
use crate::encoder::Encoder;
use crate::errors::{DecodeErrorType, DecodeResult, EncodeResult};
use crate::header::{Category, Length};
use crate::json_writer::JsonWriter;
use crate::{EncodeError, ToJsonResult};

#[derive(Debug)]
pub(crate) struct Object<'b>(ObjectChoice<'b>);

impl<'b> Object<'b> {
    pub fn decode_header(d: &mut Decoder<'b>, length: Length) -> DecodeResult<Self> {
        match length {
            Length::Empty => Ok(Self(ObjectChoice::U16(ObjectSized { super_header: &[] }))),
            Length::U32 => Ok(Self(ObjectChoice::U32(ObjectSized::new(d, length)?))),
            Length::U16 => Ok(Self(ObjectChoice::U16(ObjectSized::new(d, length)?))),
            _ => Ok(Self(ObjectChoice::U8(ObjectSized::new(d, length)?))),
        }
    }

    pub fn len(&self) -> usize {
        match &self.0 {
            ObjectChoice::U8(o) => o.len(),
            ObjectChoice::U16(o) => o.len(),
            ObjectChoice::U32(o) => o.len(),
        }
    }

    pub fn get(&self, d: &mut Decoder<'b>, key: &str) -> DecodeResult<bool> {
        match &self.0 {
            ObjectChoice::U8(o) => o.get(d, key),
            ObjectChoice::U16(o) => o.get(d, key),
            ObjectChoice::U32(o) => o.get(d, key),
        }
    }

    pub fn to_json(&self, d: &mut Decoder<'b>) -> DecodeResult<JsonObject<'b>> {
        match &self.0 {
            ObjectChoice::U8(o) => o.to_json(d),
            ObjectChoice::U16(o) => o.to_json(d),
            ObjectChoice::U32(o) => o.to_json(d),
        }
    }

    pub fn write_json(&self, d: &mut Decoder<'b>, writer: &mut JsonWriter) -> ToJsonResult<()> {
        match &self.0 {
            ObjectChoice::U8(o) => o.write_json(d, writer),
            ObjectChoice::U16(o) => o.write_json(d, writer),
            ObjectChoice::U32(o) => o.write_json(d, writer),
        }
    }
}

#[derive(Debug)]
enum ObjectChoice<'b> {
    U8(ObjectSized<'b, SuperHeaderItem8>),
    U16(ObjectSized<'b, SuperHeaderItem16>),
    U32(ObjectSized<'b, SuperHeaderItem32>),
}

#[derive(Debug)]
struct ObjectSized<'b, S: SuperHeaderItem> {
    super_header: &'b [S],
}

impl<'b, S: SuperHeaderItem> ObjectSized<'b, S> {
    fn new(d: &mut Decoder<'b>, length: Length) -> DecodeResult<Self> {
        let length = length.decode(d)?;
        let super_header: &[S] = d.take_slice_as(length)?;
        Ok(Self { super_header })
    }

    fn len(&self) -> usize {
        self.super_header.len()
    }

    fn get(&self, d: &mut Decoder<'b>, key: &str) -> DecodeResult<bool> {
        // "item" for comparison only, so offset doesn't matter. if this errors it's because the key is too long
        // to encode
        let Ok(key_item) = S::new(key, 0) else {
            return Ok(false);
        };

        let Some(header_iter) = binary_search(self.super_header, |h| h.sort_order(&key_item)) else {
            return Ok(false);
        };
        let start_index = d.index;

        for h in header_iter {
            d.index = start_index + h.offset();
            let possible_key = d.take_slice(h.key_length())?;
            if possible_key == key.as_bytes() {
                return Ok(true);
            }
        }
        // reset the index
        d.index = start_index;
        Ok(false)
    }

    fn to_json(&self, d: &mut Decoder<'b>) -> DecodeResult<JsonObject<'b>> {
        self.super_header
            .iter()
            .map(|_| {
                let key = self.take_next_key(d)?;
                let value = d.take_value()?;
                Ok((key.into(), value))
            })
            .collect::<DecodeResult<_>>()
            .map(Arc::new)
    }

    fn write_json(&self, d: &mut Decoder<'b>, writer: &mut JsonWriter) -> ToJsonResult<()> {
        let mut steps = 0..self.len();
        writer.start_object();
        if steps.next().is_some() {
            let key = self.take_next_key(d)?;
            writer.write_key(key)?;
            d.write_json(writer)?;
            for _ in steps {
                writer.comma();
                let key = self.take_next_key(d)?;
                writer.write_key(key)?;
                d.write_json(writer)?;
            }
        }
        writer.end_object();
        Ok(())
    }

    fn take_next_key(&self, d: &mut Decoder<'b>) -> DecodeResult<&'b str> {
        let header_index = S::take_header_index(d)?;
        match self.super_header.get(header_index) {
            Some(h) => d.take_str(h.key_length()),
            None => Err(d.error(DecodeErrorType::ObjectBodyIndexInvalid)),
        }
    }
}

trait SuperHeaderItem: fmt::Debug + Copy + Clone + Pod + Zeroable + Eq + PartialEq {
    fn new(key: &str, offset: usize) -> Result<Self, TryFromIntError>;

    fn sort_order(&self, other: &Self) -> Ordering;

    fn offset(&self) -> usize;

    fn key_length(&self) -> usize;

    fn header_index_le_bytes(index: usize) -> impl AsRef<[u8]>;

    fn take_header_index(d: &mut Decoder) -> DecodeResult<usize>;
}

/// `SuperHeader` Represents an item in the header
///
/// # Warning
///
/// **Member order matters here** since it decides the layout of the struct when serialized.
macro_rules! super_header_item {
    ($name:ident, $int_type:ty, $int_size:literal) => {
        #[derive(Debug, Copy, Clone, Pod, Zeroable, Eq, PartialEq)]
        #[repr(C)]
        struct $name {
            key_length: $int_type,
            key_hash: $int_type,
            offset: $int_type,
        }

        impl SuperHeaderItem for $name {
            fn new(key: &str, offset: usize) -> Result<Self, TryFromIntError> {
                Ok(Self {
                    key_length: <$int_type>::try_from(key.len())?,
                    // note we really do want key_hash to wrap around on the cast here!
                    key_hash: djb2_hash(key) as $int_type,
                    offset: <$int_type>::try_from(offset)?,
                })
            }

            fn sort_order(&self, other: &Self) -> Ordering {
                match self.key_length.cmp(&other.key_length) {
                    Ordering::Equal => self.key_hash.cmp(&other.key_hash),
                    x => x,
                }
            }

            fn offset(&self) -> usize {
                self.offset as usize
            }

            fn key_length(&self) -> usize {
                self.key_length as usize
            }

            fn header_index_le_bytes(index: usize) -> impl AsRef<[u8]> {
                let index_size = index as $int_type;
                index_size.to_le_bytes()
            }

            fn take_header_index(d: &mut Decoder) -> DecodeResult<usize> {
                // same logic as `take_<u16/u32>`
                let slice = d.take_slice($int_size)?;
                let v = <$int_type>::from_le_bytes(slice.try_into().unwrap());
                Ok(v as usize)
            }
        }
    };
}

super_header_item!(SuperHeaderItem8, u8, 1);
super_header_item!(SuperHeaderItem16, u16, 2);
super_header_item!(SuperHeaderItem32, u32, 4);

/// Search a sorted slice and return a sub-slice of values that match a given predicate.
fn binary_search<'b, S>(
    haystack: &'b [S],
    compare: impl Fn(&S) -> Ordering + 'b,
) -> Option<impl Iterator<Item = &'b S>> {
    let mut low = 0;
    let mut high = haystack.len();

    // Perform binary search to find one occurrence of the value
    loop {
        let mid = low + (high - low) / 2;
        match compare(&haystack[mid]) {
            Ordering::Less => low = mid + 1,
            Ordering::Greater => high = mid,
            Ordering::Equal => {
                // Finding the start of the sub-slice with the target value
                let start = haystack[..mid]
                    .iter()
                    .rposition(|x| compare(x).is_ne())
                    .map_or(0, |pos| pos + 1);
                return Some(haystack[start..].iter().take_while(move |x| compare(x).is_eq()));
            }
        }
        if low >= high {
            return None;
        }
    }
}

pub(crate) fn encode_object(encoder: &mut Encoder, object: &JsonObject) -> EncodeResult<()> {
    if object.is_empty() {
        // shortcut but also no alignment!
        return encoder.encode_length(Category::Object, 0);
    }

    let min_size = minimum_size_estimate(object);
    let encoder_position = encoder.position();
    if min_size <= u8::MAX as usize {
        encoder.encode_length(Category::Object, object.len())?;
        if encode_object_sized::<SuperHeaderItem8>(encoder, object)? {
            return Ok(());
        }
        encoder.reset_position(encoder_position);
    }

    if min_size <= u16::MAX as usize {
        encoder.encode_len_u16(Category::Object, u16::try_from(object.len()).unwrap());
        if encode_object_sized::<SuperHeaderItem16>(encoder, object)? {
            return Ok(());
        }
        encoder.reset_position(encoder_position);
    }

    encoder.encode_len_u32(Category::Object, object.len())?;
    if encode_object_sized::<SuperHeaderItem32>(encoder, object)? {
        Ok(())
    } else {
        Err(EncodeError::ObjectTooLarge)
    }
}

fn encode_object_sized<S: SuperHeaderItem>(encoder: &mut Encoder, object: &JsonObject) -> EncodeResult<bool> {
    let mut super_header = Vec::with_capacity(object.len());
    encoder.align::<S>();
    let super_header_start = encoder.ring_fence(object.len() * size_of::<S>());

    let offset_start = encoder.position();
    for (key, value) in object.iter() {
        let key_str = key.as_ref();
        // add space for the header index, to be set correctly later
        encoder.extend(S::header_index_le_bytes(0).as_ref());
        // push to the super header, with the position at this stage
        let Ok(h) = S::new(key_str, encoder.position() - offset_start) else {
            return Ok(false);
        };
        super_header.push(h);
        // now we've recorded the offset in the header, write the key and value to the encoder
        encoder.extend(key_str.as_bytes());
        encoder.encode_value(value)?;
    }
    super_header.sort_by(S::sort_order);

    // iterate over the super header and set the header index for each item in the body
    for (header_index, h) in super_header.iter().enumerate() {
        let header_index_bytes = S::header_index_le_bytes(header_index);
        let header_index_ref = header_index_bytes.as_ref();
        encoder.set_range(offset_start + h.offset() - header_index_ref.len(), header_index_ref);
    }
    encoder.set_range(super_header_start, bytemuck::cast_slice(&super_header));
    Ok(true)
}

/// Estimate the minimize amount of space needed to encode the object.
///
/// This is NOT recursive, instead it makes very optimistic guesses about how long arrays and objects might be.
fn minimum_size_estimate(object: &JsonObject) -> usize {
    let mut size = 0;
    for (key, value) in object.iter() {
        size += 1 + key.len(); // one byte header index and key
        size += match value {
            // we could try harder for floats, but this is a good enough for now
            JsonValue::Null | JsonValue::Bool(_) | JsonValue::Float(_) => 1,
            JsonValue::Int(i) if (0..=10).contains(i) => 1,
            // we could try harder here, but this is a good enough for now
            JsonValue::Int(_) => 2,
            JsonValue::BigInt(_) => todo!("BigInt"),
            JsonValue::Str(s) => 1 + s.len(),
            JsonValue::Array(a) => 1 + a.len(),
            JsonValue::Object(o) => 1 + o.len(),
        }
    }
    size
}

/// Very simple and fast hashing algorithm that nonetheless gives good distribution.
///
/// See <https://en.wikipedia.org/wiki/List_of_hash_functions#cite_note-Hash_functions-2> and
/// <http://www.cse.yorku.ca/~oz/hash.html> and <https://theartincode.stanis.me/008-djb2/> for more information.
fn djb2_hash(s: &str) -> u32 {
    let mut hash_value: u32 = 5381;
    for i in s.bytes() {
        // hash_value * 33 + char
        hash_value = hash_value
            .wrapping_shl(5)
            .wrapping_add(hash_value)
            .wrapping_add(u32::from(i));
    }
    hash_value
}

#[cfg(test)]
mod test {
    use jiter::JsonValue;

    use crate::header::Header;
    use crate::{compare_json_values, encode_from_json};

    use super::*;

    #[test]
    fn super_header_sizes() {
        assert_eq!(size_of::<SuperHeaderItem8>(), 3);
        assert_eq!(size_of::<SuperHeaderItem16>(), 6);
        assert_eq!(size_of::<SuperHeaderItem32>(), 12);
    }

    #[test]
    fn decode_get() {
        let v = JsonValue::Object(Arc::new(vec![
            ("aa".into(), JsonValue::Str("hello, world!".into())),
            ("bat".into(), JsonValue::Int(42)),
            ("c".into(), JsonValue::Bool(true)),
        ]));
        let b = encode_from_json(&v).unwrap();
        let mut d = Decoder::new(&b);
        let header = d.take_header().unwrap();
        assert_eq!(header, Header::Object(3.into()));

        let obj = Object::decode_header(&mut d, 3.into()).unwrap();

        assert_eq!(obj.len(), 3);

        let mut d2 = d.clone();
        assert!(obj.get(&mut d2, "aa").unwrap());
        assert_eq!(d2.take_value().unwrap(), JsonValue::Str("hello, world!".into()));

        let mut d3 = d.clone();
        assert!(obj.get(&mut d3, "bat").unwrap());
        assert_eq!(d3.take_value().unwrap(), JsonValue::Int(42));

        let mut d4 = d.clone();
        assert!(obj.get(&mut d4, "c").unwrap());
        assert_eq!(d4.take_value().unwrap(), JsonValue::Bool(true));

        assert!(!obj.get(&mut d, "x").unwrap());
    }

    #[test]
    fn offsets() {
        let v = JsonValue::Object(Arc::new(vec![
            ("a".into(), JsonValue::Bool(true)),
            (
                "bb".into(),
                JsonValue::Object(Arc::new(vec![("ccc".into(), JsonValue::Int(42))])),
            ),
        ]));
        let b = encode_from_json(&v).unwrap();
        let mut d = Decoder::new(&b);
        let header = d.take_header().unwrap();
        assert_eq!(header, Header::Object(2.into()));

        let obj = Object::decode_header(&mut d, 2.into()).unwrap();

        let obj = match obj.0 {
            ObjectChoice::U8(o) => o,
            _ => panic!("expected U8"),
        };

        assert_eq!(
            obj.super_header,
            vec![
                SuperHeaderItem8 {
                    key_length: 1,
                    key_hash: 6,
                    offset: 1
                },
                SuperHeaderItem8 {
                    key_length: 2,
                    key_hash: 73,
                    offset: 4
                }
            ]
        );

        assert!(obj.get(&mut d, "bb").unwrap());
        let header = d.take_header().unwrap();
        assert_eq!(header, Header::Object(1.into()));

        let obj = Object::decode_header(&mut d, 1.into()).unwrap();

        let obj = match obj.0 {
            ObjectChoice::U8(o) => o,
            _ => panic!("expected U8"),
        };

        assert_eq!(
            obj.super_header,
            vec![SuperHeaderItem8 {
                key_length: 3,
                key_hash: 46,
                // note the offset here is relative to the start of the object
                offset: 1,
            },]
        );
    }

    #[test]
    fn decode_empty() {
        let v = JsonValue::Object(Arc::new(Vec::new()));
        let b = encode_from_json(&v).unwrap();
        assert_eq!(b.len(), 1);
        let mut d = Decoder::new(&b);
        let header = d.take_header().unwrap();
        assert_eq!(header, Header::Object(0.into()));

        let obj = Object::decode_header(&mut d, 0.into()).unwrap();
        assert_eq!(obj.len(), 0);
    }

    #[test]
    fn binary_search_direct() {
        let slice = &["", "b", "ba", "fo", "spam"];
        let mut count = 0;
        for i in binary_search(slice, |x| x.len().cmp(&1)).unwrap() {
            assert_eq!(*i, "b");
            count += 1;
        }
        assert_eq!(count, 1);
    }

    fn binary_search_vec<S: Clone>(haystack: &[S], compare: impl Fn(&S) -> Ordering) -> Option<Vec<S>> {
        binary_search(haystack, compare).map(|i| i.cloned().collect())
    }

    #[test]
    fn binary_search_ints() {
        let slice = &[1, 2, 2, 2, 3, 4, 5, 6, 7, 7, 7, 7, 7, 7, 7, 7, 8, 8];
        assert_eq!(binary_search_vec(slice, |x| x.cmp(&1)), Some(vec![1]));
        assert_eq!(binary_search_vec(slice, |x| x.cmp(&2)), Some(vec![2, 2, 2]));
        assert_eq!(binary_search_vec(slice, |x| x.cmp(&3)), Some(vec![3]));
        assert_eq!(binary_search_vec(slice, |x| x.cmp(&4)), Some(vec![4]));
        assert_eq!(
            binary_search_vec(slice, |x| x.cmp(&7)),
            Some(vec![7, 7, 7, 7, 7, 7, 7, 7])
        );
        assert_eq!(binary_search_vec(slice, |x| x.cmp(&8)), Some(vec![8, 8]));
        assert_eq!(binary_search_vec(slice, |x| x.cmp(&12)), None);
    }

    #[test]
    fn binary_search_strings() {
        let slice = &["", "b", "ba", "fo", "spam"];
        assert_eq!(binary_search_vec(slice, |x| x.len().cmp(&0)), Some(vec![""]));
        assert_eq!(binary_search_vec(slice, |x| x.len().cmp(&1)), Some(vec!["b"]));
        assert_eq!(binary_search_vec(slice, |x| x.len().cmp(&2)), Some(vec!["ba", "fo"]));
        assert_eq!(binary_search_vec(slice, |x| x.len().cmp(&4)), Some(vec!["spam"]));
        assert_eq!(binary_search_vec(slice, |x| x.len().cmp(&5)), None);
    }

    #[test]
    fn binary_search_take_while() {
        // in valid input to test take_while isn't iterating further
        let slice = &[1, 2, 2, 1, 3];
        assert_eq!(binary_search_vec(slice, |x| x.cmp(&1)), Some(vec![1]));
    }

    #[test]
    fn exceed_size() {
        let array = JsonValue::Array(Arc::new(vec![JsonValue::Int(1_234); 100]));
        let v = Arc::new(vec![
            (
                "a".into(),
                // 240 * i64 is longer than a u8 can encode
                array.clone(),
            ),
            // need another key to encounter the error
            ("b".into(), JsonValue::Null),
        ]);

        // less than 255, so encode_from_json will try to encode with SuperHeaderItem8
        assert_eq!(minimum_size_estimate(&v), 106);
        let b = encode_from_json(&JsonValue::Object(v)).unwrap();

        let mut d = Decoder::new(&b);
        let header = d.take_header().unwrap();
        assert_eq!(header, Header::Object(Length::U16));

        let obj = Object::decode_header(&mut d, Length::U16).unwrap();
        let obj = match obj.0 {
            ObjectChoice::U16(o) => o,
            _ => panic!("expected U16"),
        };

        assert_eq!(obj.len(), 2);

        let mut d2 = d.clone();
        assert!(obj.get(&mut d2, "a").unwrap());
        assert!(compare_json_values(&d2.take_value().unwrap(), &array));

        let mut d3 = d.clone();
        assert!(obj.get(&mut d3, "b").unwrap());
        assert_eq!(d3.take_value().unwrap(), JsonValue::Null);
    }
}
