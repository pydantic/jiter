#![no_main]

use jiter::{JsonValue as JiterValue, JsonValueError as JiterError};
use serde_json::{Value as SerdeValue, Number as SerdeNumber, Error as SerdeError};

use libfuzzer_sys::fuzz_target;
use num_traits::ToPrimitive;

pub fn values_equal(jiter_value: &JiterValue, serde_value: &SerdeValue) -> bool {
    match (jiter_value, serde_value) {
        (JiterValue::Null, SerdeValue::Null) => true,
        (JiterValue::Bool(b1), SerdeValue::Bool(b2)) => b1 == b2,
        (JiterValue::Int(i1), SerdeValue::Number(n2)) => ints_equal(i1, n2),
        (JiterValue::BigInt(i1), SerdeValue::Number(n2)) => floats_approx(i1.to_f64(), n2.as_f64()),
        (JiterValue::Float(f1), SerdeValue::Number(n2)) => floats_approx(Some(*f1), n2.as_f64()),
        (JiterValue::String(s1), SerdeValue::String(s2)) => s1 == s2,
        (JiterValue::Array(a1), SerdeValue::Array(a2)) => {
            if a1.len() != a2.len() {
                return false;
            }
            for (v1, v2) in a1.iter().zip(a2.into_iter()) {
                if !values_equal(&v1, v2) {
                    return false;
                }
            }
            true
        }
        (JiterValue::Object(o1), SerdeValue::Object(o2)) => {
            if o1.len() != o2.len() {
                return false;
            }
            for (k1, v1) in o1.iter_unique() {
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

fn floats_approx(f1: Option<f64>, f2: Option<f64>) -> bool {
    match (f1, f2) {
        (Some(f1), Some(f2)) => {
            let mut threshold = f1.abs() / 1_000_000_f64;
            if threshold < 0.000_000_1 {
                threshold = 0.000_000_1;
            }
            let diff = f1 - f2;
            if diff.abs() <= threshold {
                true
            } else {
                false
            }
        },
        _ => false
    }
}

fn ints_equal(i1: &i64, n2: &SerdeNumber) -> bool {
    let i1 = *i1;
    if let Some(i2) = n2.as_i64() {
        if i1 == i2 {
            return true;
        }
    }
    return floats_approx(i1.to_f64(), n2.as_f64())
}

fn errors_equal(jiter_error: &JiterError, serde_error: &SerdeError) -> bool {
    let jiter_error_str = jiter_error.to_string();
    let serde_error_str = serde_error.to_string();
    if serde_error_str.starts_with("number out of range") {
        // ignore this case as serde is stricter so fails on this before jiter does
        true
    } else if serde_error_str.starts_with("recursion limit exceeded") {
        // serde has a different recursion limit to jiter
        true
    } else {
        return jiter_error_str == serde_error_str
    }
}

fuzz_target!(|json: String| {
    let json_data = json.as_bytes();
// fuzz_target!(|json_data: &[u8]| {
    let jiter_value = match JiterValue::parse(json_data) {
        Ok(v) => v,
        Err(jiter_error) => {
            match serde_json::from_slice::<SerdeValue>(json_data) {
                Ok(serde_value) => {
                    dbg!(json_data, serde_value, jiter_error);
                    panic!("jiter failed to parse when serde passed");
                },
                Err(serde_error) => {
                    if errors_equal(&jiter_error, &serde_error) {
                        return
                    } else {
                        println!("============================");
                        dbg!(json, &jiter_error, jiter_error.to_string(), &serde_error, serde_error.to_string());
                        panic!("errors not equal");
                        // return
                    }
                }
            }
        },
    };
    let serde_value: SerdeValue = match serde_json::from_slice(json_data) {
        Ok(v) => v,
        Err(error) => {
            let error_string = error.to_string();
            if error_string.starts_with("number out of range") {
                // this happens because of stricter behaviour on exponential floats in serde
                return
            } else {
                dbg!(error, error_string, jiter_value);
                panic!("serde_json failed to parse json that Jiter did");
            }
        },
    };

    if !values_equal(&jiter_value, &serde_value) {
        dbg!(jiter_value, serde_value);
        panic!("values not equal");
    }
});
