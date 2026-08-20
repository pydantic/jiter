//! Compare `jiter` with `serde_json` over the json-cases corpus: the two must agree on whether
//! each document is valid JSON and on the value it decodes to, bar the differences
//! [`known_difference`] lists. [`similar_cases_agree`] separately holds jiter to the corpus's
//! `similar` groups, one document spelled several ways: every member decodes to the same value, or
//! they are all rejected. See [`corpus`] for the checkout these need.
// floats are compared exactly on purpose, see `numbers_equal`
#![allow(clippy::float_cmp, clippy::print_stdout)]

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::fmt::Write as _;

use jiter::JsonValue;
use serde_json::Value as SerdeValue;

mod corpus;

use corpus::{Loaded, corpus_root, load_cases};

/// How jiter and serde_json disagreed about one document.
#[derive(Debug)]
enum Mismatch {
    /// jiter parsed the document, serde_json rejected it
    JiterOnly { serde_error: String },
    /// serde_json parsed the document, jiter rejected it
    SerdeOnly { jiter_error: String },
    /// both parsed it, but into different values
    Value { detail: String },
    /// both rejected it, but with different errors
    Error { jiter_error: String, serde_error: String },
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::JiterOnly { serde_error } => {
                write!(f, "jiter parsed it, serde_json returned an error: {serde_error:?}")
            }
            Self::SerdeOnly { jiter_error } => {
                write!(f, "serde_json parsed it, jiter returned an error: {jiter_error:?}")
            }
            Self::Value { detail } => write!(f, "both parsed it, {detail}"),
            Self::Error {
                jiter_error,
                serde_error,
            } => {
                write!(
                    f,
                    "both rejected it, jiter returned an error: {jiter_error:?}, serde_json returned an error: {serde_error:?}"
                )
            }
        }
    }
}

/// The ways jiter is known to differ from serde_json, and why; anything else is a test failure.
fn known_difference(mismatch: &Mismatch) -> Option<&'static str> {
    match mismatch {
        // jiter refuses integers and floats with more than 4300 digits, matching CPython's default
        // `sys.set_int_max_str_digits()`; serde_json (built with `arbitrary_precision` here) keeps
        // the raw token, so it has no such limit
        Mismatch::SerdeOnly { jiter_error } if starts_with_msg(jiter_error, "number out of range") => {
            Some("jiter caps numbers at 4300 digits, serde_json does not")
        }
        // jiter's recursion limit is 200, serde_json's default is 128, so documents nested between
        // the two are accepted by jiter alone, and documents that are also malformed hit one limit
        // or the other first
        Mismatch::JiterOnly { serde_error } if starts_with_msg(serde_error, "recursion limit exceeded") => {
            Some("jiter's recursion limit is 200, serde_json's is 128")
        }
        Mismatch::Error {
            jiter_error,
            serde_error,
        } if starts_with_msg(jiter_error, "recursion limit exceeded")
            || starts_with_msg(serde_error, "recursion limit exceeded") =>
        {
            Some("jiter's recursion limit is 200, serde_json's is 128")
        }
        // the two agree that the document is malformed and on why, but not on where: serde_json
        // reports the start of the string for an invalid code point (serde-rs/json#1083), and the
        // two count the start of an invalid escape differently (pydantic/jiter#130)
        Mismatch::Error {
            jiter_error,
            serde_error,
        } if without_position(jiter_error) == without_position(serde_error)
            && matches!(
                without_position(jiter_error),
                "invalid unicode code point" | "invalid escape"
            ) =>
        {
            Some("same error, different position")
        }
        _ => None,
    }
}

/// Both parsers suffix their messages with ` at line N column M`; this is the message alone.
fn without_position(error: &str) -> &str {
    match error.find(" at line ") {
        Some(index) => &error[..index],
        None => error,
    }
}

fn starts_with_msg(error: &str, msg: &str) -> bool {
    without_position(error) == msg
}

/// Parse `json_data` with both parsers, returning how they disagreed, if they did.
fn compare(json_data: &[u8], allow_inf_nan: bool) -> Option<Mismatch> {
    match (
        JsonValue::parse(json_data, allow_inf_nan),
        serde_json::from_slice::<SerdeValue>(json_data),
    ) {
        (Ok(jiter_value), Ok(serde_value)) => {
            let mut path = String::new();
            let equal = values_equal(&jiter_value, &serde_value, &mut path);
            (!equal).then_some(Mismatch::Value { detail: path })
        }
        (Ok(_), Err(serde_error)) => Some(Mismatch::JiterOnly {
            serde_error: serde_error.to_string(),
        }),
        (Err(jiter_error), Ok(_)) => Some(Mismatch::SerdeOnly {
            jiter_error: jiter_error.description(json_data),
        }),
        (Err(jiter_error), Err(serde_error)) => {
            let jiter_error = jiter_error.description(json_data);
            let serde_error = serde_error.to_string();
            (jiter_error != serde_error).then_some(Mismatch::Error {
                jiter_error,
                serde_error,
            })
        }
    }
}

/// Compare a jiter value with a serde_json one, describing where they first differ in `detail`.
fn values_equal(jiter_value: &JsonValue, serde_value: &SerdeValue, detail: &mut String) -> bool {
    match (jiter_value, serde_value) {
        (JsonValue::Null, SerdeValue::Null) => true,
        (JsonValue::Bool(b1), SerdeValue::Bool(b2)) => b1 == b2,
        (JsonValue::Str(s1), SerdeValue::String(s2)) => s1 == s2,
        (JsonValue::Int(_) | JsonValue::Float(_), SerdeValue::Number(n)) => {
            numbers_equal(jiter_value, &n.to_string(), detail)
        }
        #[cfg(feature = "num-bigint")]
        (JsonValue::BigInt(_), SerdeValue::Number(n)) => numbers_equal(jiter_value, &n.to_string(), detail),
        (JsonValue::Array(a1), SerdeValue::Array(a2)) => {
            if a1.len() != a2.len() {
                let _ = write!(detail, "array has {} elements, serde_json got {}", a1.len(), a2.len());
                return false;
            }
            for (index, (v1, v2)) in a1.iter().zip(a2.iter()).enumerate() {
                if !values_equal(v1, v2, detail) {
                    detail.insert_str(0, &format!("[{index}]"));
                    return false;
                }
            }
            true
        }
        (JsonValue::Object(o1), SerdeValue::Object(o2)) => {
            let o1 = deduplicate(o1);
            if o1.len() != o2.len() {
                let _ = write!(detail, "object has {} keys, serde_json got {}", o1.len(), o2.len());
                return false;
            }
            for ((k1, v1), (k2, v2)) in o1.iter().zip(o2.iter()) {
                if k1 != k2 {
                    let _ = write!(detail, "key {k1:?}, serde_json got {k2:?}");
                    return false;
                }
                if !values_equal(v1, v2, detail) {
                    detail.insert_str(0, &format!("[{k1:?}]"));
                    return false;
                }
            }
            true
        }
        _ => {
            let _ = write!(detail, "{jiter_value:?}, serde_json got {serde_value:?}");
            false
        }
    }
}

/// The object's members with duplicate keys collapsed the way serde_json's map collapses them —
/// the first position a key appeared at, holding the last value given for it. jiter doesn't
/// deduplicate while parsing, it leaves that to the caller.
fn deduplicate<'a, 'j>(object: &'a [(Cow<'j, str>, JsonValue<'j>)]) -> Vec<(&'a str, &'a JsonValue<'j>)> {
    let mut positions: HashMap<&str, usize> = HashMap::with_capacity(object.len());
    let mut members: Vec<(&str, &JsonValue)> = Vec::with_capacity(object.len());
    for (key, value) in object {
        if let Some(&index) = positions.get(key.as_ref()) {
            members[index].1 = value;
        } else {
            positions.insert(key.as_ref(), members.len());
            members.push((key.as_ref(), value));
        }
    }
    members
}

/// serde_json is built with `arbitrary_precision` in this crate's dev-dependencies, so its numbers
/// are the raw token; compare jiter's decoded number with that token rather than with a second
/// lossy decode of it.
fn numbers_equal(jiter_value: &JsonValue, serde_number: &str, detail: &mut String) -> bool {
    let equal = match jiter_value {
        JsonValue::Int(i) => serde_number.parse::<i64>().is_ok_and(|s| s == *i),
        #[cfg(feature = "num-bigint")]
        JsonValue::BigInt(b) => serde_number.parse::<num_bigint::BigInt>().is_ok_and(|s| &s == b),
        // both decode with correct rounding, so this is exact rather than approximate
        JsonValue::Float(f) => serde_number.parse::<f64>().is_ok_and(|s| s == *f),
        _ => false,
    };
    if !equal {
        let _ = write!(detail, "{jiter_value:?}, serde_json got the token {serde_number}");
    }
    equal
}

/// Every case in the corpus, parsed by jiter and by serde_json, must give the same answer.
#[test]
fn compare_to_serde_json() {
    let Some(root) = corpus_root() else {
        return;
    };
    let mut unexpected: Vec<String> = Vec::new();
    let mut known: HashMap<&'static str, usize> = HashMap::new();
    let cases = load_cases(&root);
    for case in &cases {
        // `allow_inf_nan` is off so that jiter is asked for the same language serde_json parses;
        // `inf_nan_extension` below covers what turning it on adds
        if let Some(mismatch) = compare(&case.json_data, false) {
            match known_difference(&mismatch) {
                Some(reason) => *known.entry(reason).or_default() += 1,
                None => unexpected.push(format!("{}: {mismatch}", case.name)),
            }
        }
    }

    println!("{} cases", cases.len());
    let mut known: Vec<_> = known.into_iter().collect();
    known.sort_unstable();
    for (reason, count) in known {
        println!("  {count} known differences: {reason}");
    }
    assert!(
        unexpected.is_empty(),
        "{} of {} cases parsed differently by jiter and serde_json:\n{}",
        unexpected.len(),
        cases.len(),
        unexpected.join("\n")
    );
}

/// With `allow_inf_nan` jiter also accepts the `NaN`/`Infinity` documents CPython's `json` module
/// writes and reads, which are not JSON — the one place jiter deliberately parses more than
/// serde_json does. It must add nothing else: everything serde_json accepts must still parse to
/// the same value.
#[test]
fn inf_nan_extension() {
    let Some(root) = corpus_root() else {
        return;
    };
    let mut extra: Vec<String> = Vec::new();
    let mut unexpected: Vec<String> = Vec::new();
    let cases = load_cases(&root);
    let inf_nan = cases.iter().filter(|case| case.category == "python-inf-nan");
    for case in inf_nan {
        match compare(&case.json_data, true) {
            Some(Mismatch::JiterOnly { .. }) => {
                // the only documents jiter may accept and serde_json reject are the ones this
                // directory exists for
                let text = String::from_utf8_lossy(&case.json_data);
                assert!(
                    text.contains("NaN") || text.contains("Infinity"),
                    "{}: accepted with allow_inf_nan but holds no NaN/Infinity",
                    case.name
                );
                extra.push(case.name.clone());
            }
            // both rejected it: jiter reads further into a malformed `NaN`/`Infinity` token
            // before giving up than serde_json does, so only the verdict has to agree here
            Some(Mismatch::Error { .. }) | None => (),
            // jiter must not reject, or decode differently, anything serde_json accepts, unless
            // it is one of the differences the corpus-wide test already allows
            Some(mismatch) if known_difference(&mismatch).is_none() => {
                unexpected.push(format!("{}: {mismatch}", case.name));
            }
            Some(_) => (),
        }
    }

    assert!(unexpected.is_empty(), "{}", unexpected.join("\n"));
    assert!(
        !extra.is_empty(),
        "no NaN/Infinity documents were accepted, is the corpus complete?"
    );
    println!("{} NaN/Infinity documents accepted with allow_inf_nan", extra.len());
}

/// Compare two values jiter decoded, describing where they first differ in `detail`. Members of a
/// group hold the same numbers written the same way and the same keys in the same order, so this
/// is exact, structural equality; `NaN`, which is not equal to itself, is the one special case.
fn jiter_values_equal(v1: &JsonValue, v2: &JsonValue, detail: &mut String) -> bool {
    match (v1, v2) {
        (JsonValue::Float(f1), JsonValue::Float(f2)) if f1.is_nan() && f2.is_nan() => true,
        (JsonValue::Array(a1), JsonValue::Array(a2)) => {
            if a1.len() != a2.len() {
                let _ = write!(detail, "array has {} elements, the other has {}", a1.len(), a2.len());
                return false;
            }
            for (index, (e1, e2)) in a1.iter().zip(a2.iter()).enumerate() {
                if !jiter_values_equal(e1, e2, detail) {
                    detail.insert_str(0, &format!("[{index}]"));
                    return false;
                }
            }
            true
        }
        // jiter doesn't deduplicate keys, and neither does the corpus: a group repeats the same
        // key the same number of times, so the members are compared as written
        (JsonValue::Object(o1), JsonValue::Object(o2)) => {
            if o1.len() != o2.len() {
                let _ = write!(detail, "object has {} keys, the other has {}", o1.len(), o2.len());
                return false;
            }
            for ((k1, e1), (k2, e2)) in o1.iter().zip(o2.iter()) {
                if k1 != k2 {
                    let _ = write!(detail, "key {k1:?}, the other has {k2:?}");
                    return false;
                }
                if !jiter_values_equal(e1, e2, detail) {
                    detail.insert_str(0, &format!("[{k1:?}]"));
                    return false;
                }
            }
            true
        }
        _ => {
            if v1 == v2 {
                return true;
            }
            let _ = write!(detail, "{v1:?}, the other has {v2:?}");
            false
        }
    }
}

/// The members of a `similar` group are one document written several ways — re-indented, or with
/// its strings escaped differently — so jiter must decode them all to the same value, or, where
/// the document is malformed, reject them all. Only *where* the error is reported may vary within
/// a rejected group, the spellings put the same mistake in different places.
#[test]
fn similar_cases_agree() {
    let Some(root) = corpus_root() else {
        return;
    };
    let cases = load_cases(&root);
    let by_name: HashMap<&str, &Loaded> = cases.iter().map(|case| (case.name.as_str(), case)).collect();
    let mut failures: Vec<String> = Vec::new();
    let mut groups = 0;
    let mut documents = 0;

    for case in cases.iter().filter(|case| !case.similar.is_empty()) {
        groups += 1;
        documents += case.similar.len() + 1;
        // what a group promises is a property of the document, not of how jiter is configured, so
        // it has to hold either way; `allow_inf_nan` is also what makes the `python-inf-nan`
        // groups compare values at all rather than agree on rejecting them
        for allow_inf_nan in [false, true] {
            let first = JsonValue::parse(&case.json_data, allow_inf_nan);
            for name in &case.similar {
                let other = by_name
                    .get(name.as_str())
                    .unwrap_or_else(|| panic!("{name}: listed as similar but missing from cases.json"));
                match (&first, JsonValue::parse(&other.json_data, allow_inf_nan)) {
                    (Ok(value), Ok(other_value)) => {
                        let mut detail = String::new();
                        if !jiter_values_equal(value, &other_value, &mut detail) {
                            failures.push(format!(
                                "{} and {} (allow_inf_nan={allow_inf_nan}) decoded differently: {detail}",
                                case.name, other.name
                            ));
                        }
                    }
                    // both rejected, which is all a group of malformed documents promises
                    (Err(_), Err(_)) => (),
                    (Ok(_), Err(error)) => failures.push(format!(
                        "{} parsed (allow_inf_nan={allow_inf_nan}), {} was rejected: {}",
                        case.name,
                        other.name,
                        error.description(&other.json_data)
                    )),
                    (Err(error), Ok(_)) => failures.push(format!(
                        "{} was rejected (allow_inf_nan={allow_inf_nan}): {}, {} parsed",
                        case.name,
                        error.description(&case.json_data),
                        other.name
                    )),
                }
            }
        }
    }

    assert!(
        groups > 0,
        "cases.json holds no `similar` groups, is the corpus complete?"
    );
    assert!(
        failures.is_empty(),
        "{} comparisons across {groups} similar groups disagreed:\n{}",
        failures.len(),
        failures.join("\n")
    );
    println!("{groups} similar groups, {documents} documents");
}
