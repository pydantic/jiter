use std::cmp::Ordering;
use std::fmt;
use std::mem::size_of;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use jiter::JsonObject;

use crate::decoder::Decoder;
use crate::encoder::Encoder;
use crate::errors::{DecodeErrorType, DecodeResult, EncodeResult};
use crate::header::{Category, Length};
use crate::json_writer::JsonWriter;
use crate::ToJsonResult;

#[derive(Debug)]
pub(crate) struct Object<'b>(ObjectChoice<'b>);

impl<'b> Object<'b> {
    pub fn decode_header(d: &mut Decoder<'b>, length: Length) -> DecodeResult<Self> {
        match length {
            Length::Empty => Ok(Self(ObjectChoice::U16(ObjectSized { super_header: &[] }))),
            Length::U8 | Length::U16 | Length::U32 => Ok(Self(ObjectChoice::U32(ObjectSized::new(d, length)?))),
            _ => Ok(Self(ObjectChoice::U16(ObjectSized::new(d, length)?))),
        }
    }

    pub fn len(&self) -> usize {
        match &self.0 {
            ObjectChoice::U16(o) => o.len(),
            ObjectChoice::U32(o) => o.len(),
        }
    }

    pub fn get(&self, d: &mut Decoder<'b>, key: &str) -> DecodeResult<bool> {
        match &self.0 {
            ObjectChoice::U16(o) => o.get(d, key),
            ObjectChoice::U32(o) => o.get(d, key),
        }
    }

    pub fn to_json(&self, d: &mut Decoder<'b>) -> DecodeResult<JsonObject<'b>> {
        match &self.0 {
            ObjectChoice::U16(o) => o.to_json(d),
            ObjectChoice::U32(o) => o.to_json(d),
        }
    }

    pub fn write_json(&self, d: &mut Decoder<'b>, writer: &mut JsonWriter) -> ToJsonResult<()> {
        match &self.0 {
            ObjectChoice::U16(o) => o.write_json(d, writer),
            ObjectChoice::U32(o) => o.write_json(d, writer),
        }
    }
}

#[derive(Debug)]
enum ObjectChoice<'b> {
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
        // "item" for comparison only
        let key_item = S::for_sort(key);
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
    fn new(key: &str, offset: usize) -> Self;

    /// For use with `sort_order`, so offset doesn't matter
    fn for_sort(key: &str) -> Self {
        Self::new(key, 0)
    }

    fn sort_order(&self, other: &Self) -> Ordering;

    fn offset(&self) -> usize;

    fn key_length(&self) -> usize;

    fn header_index_le_bytes(index: usize) -> Vec<u8>;

    fn take_header_index(d: &mut Decoder) -> DecodeResult<usize>;
}

/// `SuperHeader` Represents an item in the header
///
/// # Warning
///
/// **Member order matters here** since it decides the layout of the struct when serialized.
macro_rules! super_header_item {
    ($name:ident, $size:ty, $take_func:literal) => {
        #[derive(Debug, Copy, Clone, Pod, Zeroable, Eq, PartialEq)]
        #[repr(C)]
        struct $name {
            key_length: $size,
            key_hash: $size,
            offset: $size,
        }

        impl SuperHeaderItem for $name {
            fn new(key: &str, offset: usize) -> Self {
                Self {
                    key_length: key.len() as $size,
                    key_hash: djb2_hash(key) as $size,
                    offset: offset as $size,
                }
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

            /// This is ugly
            /// annoyingly we have to return a Vec here since we can't return a generic array or a slice
            /// I tried doing something complex with generic consts, but it got too complicated
            fn header_index_le_bytes(index: usize) -> Vec<u8> {
                let index_size = index as $size;
                index_size.to_le_bytes().to_vec()
            }

            fn take_header_index(d: &mut Decoder) -> DecodeResult<usize> {
                match $take_func {
                    "take_u16" => d.take_u16().map(|v| v as usize),
                    "take_u32" => d.take_u32().map(|v| v as usize),
                    _ => unreachable!("invalid take_func"),
                }
            }
        }
    };
}

super_header_item!(SuperHeaderItem16, u16, "take_u16");
super_header_item!(SuperHeaderItem32, u32, "take_u32");

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
        encoder.encode_length(Category::Object, 0)
    } else if object.len() <= 10 {
        // TODO this logic needs to be improved, and we need to fall back to the larger size if the
        encode_object_sized::<SuperHeaderItem16>(encoder, object)
    } else {
        encode_object_sized::<SuperHeaderItem32>(encoder, object)
    }
}

fn encode_object_sized<S: SuperHeaderItem>(encoder: &mut Encoder, object: &JsonObject) -> EncodeResult<()> {
    encoder.encode_length(Category::Object, object.len())?;

    let mut super_header = Vec::with_capacity(object.len());
    encoder.align::<S>();
    let super_header_start = encoder.ring_fence(object.len() * size_of::<S>());

    let offset_start = encoder.position();
    for (key, value) in object.iter() {
        let key_str = key.as_ref();
        // add space for the header index, to be set correctly later
        encoder.extend(&S::header_index_le_bytes(0));
        // push to the super header, with the position at this stage
        super_header.push(S::new(key_str, encoder.position() - offset_start));
        // now we've recorded the position, write the key and value to the encoder
        encoder.extend(key_str.as_bytes());
        encoder.encode_value(value)?;
    }
    super_header.sort_by(S::sort_order);

    // iterate over the super header and set the header index for each item in the body
    for (header_index, h) in super_header.iter().enumerate() {
        let header_index_bytes = S::header_index_le_bytes(header_index);
        encoder.set_range(
            offset_start + h.offset() - header_index_bytes.len(),
            &header_index_bytes,
        );
    }
    encoder.set_range(super_header_start, bytemuck::cast_slice(&super_header));
    Ok(())
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

    use crate::encode_from_json;
    use crate::header::Header;

    use super::*;

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
        let v = JsonValue::Object(Arc::new(LazyIndexMap::from(vec![
            ("a".into(), JsonValue::Bool(true)),
            (
                "bb".into(),
                JsonValue::Object(Arc::new(LazyIndexMap::from(vec![("ccc".into(), JsonValue::Int(42))]))),
            ),
        ])));
        let b = encode_from_json(&v).unwrap();
        let mut d = Decoder::new(&b);
        let header = d.take_header().unwrap();
        assert_eq!(header, Header::Object(2.into()));

        let obj = Object::decode_header(&mut d, 2.into()).unwrap();

        let obj = match obj.0 {
            ObjectChoice::U16(o) => o,
            _ => panic!("expected U16"),
        };

        assert_eq!(
            obj.super_header,
            vec![
                SuperHeaderItem16 {
                    key_length: 1,
                    key_hash: 46598,
                    offset: 2
                },
                SuperHeaderItem16 {
                    key_length: 2,
                    key_hash: 30537,
                    offset: 6
                }
            ]
        );

        assert!(obj.get(&mut d, "bb").unwrap());
        let header = d.take_header().unwrap();
        assert_eq!(header, Header::Object(1.into()));

        let obj = Object::decode_header(&mut d, 1.into()).unwrap();

        let obj = match obj.0 {
            ObjectChoice::U16(o) => o,
            _ => panic!("expected U16"),
        };

        dbg!(obj.super_header);
        assert_eq!(
            obj.super_header,
            vec![SuperHeaderItem16 {
                key_length: 3,
                key_hash: 25902,
                // note the offset here is relative to the start of the object
                offset: 2,
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
}
