#![no_main]

use std::fmt;

use jiter::JsonValue;
use num_bigint::BigInt;
use indexmap::IndexMap;
use serde::de::{Deserialize, DeserializeSeed, Error as SerdeError, MapAccess, SeqAccess, Visitor};

use libfuzzer_sys::fuzz_target;

#[derive(Clone, Debug, PartialEq)]
pub enum SerdeJsonValue {
    Null,
    Bool(bool),
    Int(i64),
    BigInt(BigInt),
    Float(f64),
    String(String),
    Array(JsonArray),
    Object(JsonObject),
}
pub type JsonArray = Vec<SerdeJsonValue>;
pub type JsonObject = IndexMap<String, SerdeJsonValue>;

fn values_equal(jiter_value: &JsonValue, serde_value: &SerdeJsonValue) -> bool {
    match (jiter_value, serde_value) {
        (JsonValue::Null, SerdeJsonValue::Null) => true,
        (JsonValue::Bool(b1), SerdeJsonValue::Bool(b2)) => b1 == b2,
        (JsonValue::Int(i1), SerdeJsonValue::Int(i2)) => i1 == i2,
        (JsonValue::BigInt(i1), SerdeJsonValue::BigInt(i2)) => i1 == i2,
        // (JsonValue::Float(f1), SerdeJsonValue::Float(f2)) => f1 == f2,
        (JsonValue::Float(f1), SerdeJsonValue::Float(f2)) => (f1 - f2).abs() < 0.000000000000001,
        (JsonValue::String(s1), SerdeJsonValue::String(s2)) => s1 == s2,
        (JsonValue::Array(a1), SerdeJsonValue::Array(a2)) => {
            if a1.len() != a2.len() {
                return false;
            }
            for (v1, v2) in a1.into_iter().zip(a2.into_iter()) {
                if !values_equal(v1, v2) {
                    return false;
                }
            }
            true
        }
        (JsonValue::Object(o1), SerdeJsonValue::Object(o2)) => {
            if o1.len() != o2.len() {
                return false;
            }
            for (k1, v1) in o1.into_iter() {
                if let Some(v2) = o2.get(k1) {
                    if !values_equal(v1, v2) {
                        return false;
                    }
                } else {
                    return false;
                }
            }
            true
        },
        _ => false,
    }
}

impl<'de> Deserialize<'de> for SerdeJsonValue {
    fn deserialize<D>(deserializer: D) -> Result<SerdeJsonValue, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct JsonVisitor;

        impl<'de> Visitor<'de> for JsonVisitor {
            type Value = SerdeJsonValue;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("any valid JSON value")
            }

            fn visit_bool<E>(self, value: bool) -> Result<SerdeJsonValue, E> {
                Ok(SerdeJsonValue::Bool(value))
            }

            fn visit_i64<E>(self, value: i64) -> Result<SerdeJsonValue, E> {
                Ok(SerdeJsonValue::Int(value))
            }

            fn visit_u64<E>(self, value: u64) -> Result<SerdeJsonValue, E> {
                match i64::try_from(value) {
                    Ok(i) => Ok(SerdeJsonValue::Int(i)),
                    Err(_) => Ok(SerdeJsonValue::BigInt(value.into())),
                }
            }

            fn visit_f64<E>(self, value: f64) -> Result<SerdeJsonValue, E> {
                Ok(SerdeJsonValue::Float(value))
            }

            fn visit_str<E>(self, value: &str) -> Result<SerdeJsonValue, E>
            where
                E: SerdeError,
            {
                Ok(SerdeJsonValue::String(value.to_string()))
            }

            fn visit_string<E>(self, value: String) -> Result<SerdeJsonValue, E> {
                Ok(SerdeJsonValue::String(value))
            }

            fn visit_none<E>(self) -> Result<SerdeJsonValue, E> {
                unreachable!()
            }

            fn visit_some<D>(self, _: D) -> Result<SerdeJsonValue, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                unreachable!()
            }

            fn visit_unit<E>(self) -> Result<SerdeJsonValue, E> {
                Ok(SerdeJsonValue::Null)
            }

            fn visit_seq<V>(self, mut visitor: V) -> Result<SerdeJsonValue, V::Error>
            where
                V: SeqAccess<'de>,
            {
                let mut vec = Vec::new();

                while let Some(elem) = visitor.next_element()? {
                    vec.push(elem);
                }

                Ok(SerdeJsonValue::Array(vec))
            }

            fn visit_map<V>(self, mut visitor: V) -> Result<SerdeJsonValue, V::Error>
            where
                V: MapAccess<'de>,
            {
                const SERDE_JSON_NUMBER: &str = "$serde_json::private::Number";
                match visitor.next_key_seed(KeyDeserializer)? {
                    Some(first_key) => {
                        let mut values = IndexMap::new();
                        let first_value = visitor.next_value()?;

                        // serde_json will parse arbitrary precision numbers into a map
                        // structure with a "number" key and a String value
                        'try_number: {
                            if first_key == SERDE_JSON_NUMBER {
                                // Just in case someone tries to actually store that key in a real map,
                                // keep parsing and continue as a map if so

                                if let Some((key, value)) = visitor.next_entry::<String, SerdeJsonValue>()? {
                                    // Important to preserve order of the keys
                                    values.insert(first_key, first_value);
                                    values.insert(key, value);
                                    break 'try_number;
                                }

                                if let SerdeJsonValue::String(s) = &first_value {
                                    // Normalize the string to either an int or float
                                    let normalized = if s.chars().any(|c| c == '.' || c == 'E' || c == 'e') {
                                        SerdeJsonValue::Float(
                                            s.parse()
                                                .map_err(|e| V::Error::custom(format!("expected a float: {e}")))?,
                                        )
                                    } else if let Ok(i) = s.parse::<i64>() {
                                        SerdeJsonValue::Int(i)
                                    } else if let Ok(big) = s.parse::<BigInt>() {
                                        SerdeJsonValue::BigInt(big)
                                    } else {
                                        // Failed to normalize, just throw it in the map and continue
                                        values.insert(first_key, first_value);
                                        break 'try_number;
                                    };

                                    return Ok(normalized);
                                };
                            } else {
                                values.insert(first_key, first_value);
                            }
                        }

                        while let Some((key, value)) = visitor.next_entry()? {
                            values.insert(key, value);
                        }
                        Ok(SerdeJsonValue::Object(values))
                    }
                    None => Ok(SerdeJsonValue::Object(IndexMap::new())),
                }
            }
        }

        deserializer.deserialize_any(JsonVisitor)
    }
}

struct KeyDeserializer;

impl<'de> DeserializeSeed<'de> for KeyDeserializer {
    type Value = String;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(self)
    }
}

impl<'de> Visitor<'de> for KeyDeserializer {
    type Value = String;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a string key")
    }

    fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(s.to_string())
    }

    fn visit_string<E>(self, _: String) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        unreachable!()
    }
}

fuzz_target!(|json: String| {
    let json_data = json.as_bytes();
    let jiter_value = match JsonValue::parse(json_data) {
        Ok(v) => v,
        Err(e) => {
            match serde_json::from_slice::<SerdeJsonValue>(json_data) {
                Ok(_) => panic!("jiter failed to parse: {:?}: {:?}", json, e),
                Err(_) => return,
            }
        },
    };
    let serde_value: SerdeJsonValue = match serde_json::from_slice(json_data) {
        Ok(v) => v,
        // Err(_) => panic!("serde_json failed to parse json: {:?}", json_data),
        Err(_) => return,
    };

    if !values_equal(&jiter_value, &serde_value) {
        panic!("values not equal: {:?} {:?}", jiter_value, serde_value);
    }
});
