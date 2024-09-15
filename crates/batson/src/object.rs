use std::cmp::Ordering;
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
pub(crate) struct Object<'b> {
    super_header: &'b [SuperHeaderItem],
}

impl<'b> Object<'b> {
    pub fn decode_header(d: &mut Decoder<'b>, length: Length) -> DecodeResult<Self> {
        if matches!(length, Length::Empty) {
            Ok(Self { super_header: &[] })
        } else {
            let length = length.decode(d)?;
            let super_header = d.take_slice_as(length)?;
            Ok(Self { super_header })
        }
    }

    pub fn len(&self) -> usize {
        self.super_header.len()
    }

    pub fn get(&self, d: &mut Decoder<'b>, key: &str) -> DecodeResult<bool> {
        // key length and hash
        let kl = key.len() as u32;
        let kh = djb2_hash(key);
        let Some(header_iter) = binary_search(self.super_header, |h| h.cmp_raw(kl, kh)) else {
            return Ok(false);
        };
        let start_index = d.index;

        for h in header_iter {
            d.index = start_index + h.offset as usize;
            let possible_key = d.take_slice(h.key_length as usize)?;
            if possible_key == key.as_bytes() {
                return Ok(true);
            }
        }
        // reset the index
        d.index = start_index;
        Ok(false)
    }

    pub fn to_json(&self, d: &mut Decoder<'b>) -> DecodeResult<JsonObject<'b>> {
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

    pub fn write_json(&self, d: &mut Decoder<'b>, writer: &mut JsonWriter) -> ToJsonResult<()> {
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

    pub fn take_next_key(&self, d: &mut Decoder<'b>) -> DecodeResult<&'b str> {
        let header_index = d.take_u32()?;
        match self.super_header.get(header_index as usize) {
            Some(h) => d.take_str(h.key_length as usize),
            None => Err(d.error(DecodeErrorType::ObjectBodyIndexInvalid)),
        }
    }
}

/// Represents an item in the header
///
/// # Warning
///
/// **Member order matters here** since it decides the layout of the struct when serialized.
#[derive(Debug, Copy, Clone, Pod, Zeroable, Eq, PartialEq)]
#[repr(C)]
struct SuperHeaderItem {
    key_length: u32,
    key_hash: u32,
    offset: u32,
}

impl SuperHeaderItem {
    fn new(key: &str, offset: u32) -> Self {
        Self {
            key_length: key.len() as u32,
            key_hash: djb2_hash(key),
            offset,
        }
    }

    fn cmp_raw(&self, key_len: u32, key_hash: u32) -> Ordering {
        match self.key_length.cmp(&key_len) {
            Ordering::Equal => self.key_hash.cmp(&key_hash),
            x => x,
        }
    }
}

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

    encoder.encode_length(Category::Object, object.len())?;

    let mut super_header = Vec::with_capacity(object.len());
    encoder.align::<SuperHeaderItem>();
    let super_header_start = encoder.ring_fence(object.len() * size_of::<SuperHeaderItem>());

    let offset_start = encoder.position();
    for (key, value) in object.iter() {
        let key_str = key.as_ref();
        // add space for the header index, to be set correctly later
        encoder.extend(&0u32.to_le_bytes());
        // push to the super header, with the position at this stage
        super_header.push(SuperHeaderItem::new(
            key_str,
            (encoder.position() - offset_start) as u32,
        ));
        // now we've recorded the position, write the key and value to the encoder
        encoder.extend(key_str.as_bytes());
        encoder.encode_value(value)?;
    }
    super_header.sort_by(|a, b| a.cmp_raw(b.key_length, b.key_hash));

    // iterate over the super header and set the header index for each item in the body
    for (header_index, h) in super_header.iter().enumerate() {
        encoder.set_range(
            offset_start + h.offset as usize - 4,
            &(header_index as u32).to_le_bytes(),
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

        assert_eq!(
            obj.super_header,
            vec![
                SuperHeaderItem {
                    key_length: 1,
                    key_hash: 177670,
                    offset: 4
                },
                SuperHeaderItem {
                    key_length: 2,
                    key_hash: 5863241,
                    offset: 10
                }
            ]
        );

        assert!(obj.get(&mut d, "bb").unwrap());
        let header = d.take_header().unwrap();
        assert_eq!(header, Header::Object(1.into()));

        let obj = Object::decode_header(&mut d, 1.into()).unwrap();

        dbg!(obj.super_header);
        assert_eq!(
            obj.super_header,
            vec![SuperHeaderItem {
                key_length: 3,
                key_hash: 193488174,
                // note the offset here is relative to the start of the object
                offset: 4,
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
