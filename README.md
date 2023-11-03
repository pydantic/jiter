# jiter

[![CI](https://github.com/pydantic/jiter/actions/workflows/ci.yml/badge.svg?event=push)](https://github.com/pydantic/jiter/actions/workflows/ci.yml?query=branch%3Amain)
[![Crates.io](https://img.shields.io/crates/v/jiter?color=green)](https://crates.io/crates/jiter)

Fast iterable JSON parser.

jiter has two interfaces:
* [JsonValue] an enum representing JSON data
* [Jiter] an iterator over JSON data
* [python_parse] which parses a JSON string into a Python object

## JsonValue Example

See [the docs][JsonValue] for more details.

```rust
use jiter::JsonValue;

fn main() {
    let json_data = r#"
        {
            "name": "John Doe",
            "age": 43,
            "phones": [
                "+44 1234567",
                "+44 2345678"
            ]
        }"#;
    let json_value = JsonValue::parse(json_data.as_bytes(), true).unwrap();
    println!("{:#?}", json_value);
}
```

returns:

```text
Object(
    {
        "name": Str("John Doe"),
        "age": Int(43),
        "phones": Array(
            [
                Str("+44 1234567"),
                Str("+44 2345678"),
            ],
        ),
    },
)
```

## Jiter Example

To use [Jiter], you need to know what schema you're expecting:

```rust
use jiter::{Jiter, NumberInt, Peak};

fn main() {
    let json_data = r#"
        {
            "name": "John Doe",
            "age": 43,
            "phones": [
                "+44 1234567",
                "+44 2345678"
            ]
        }"#;
    let mut jiter = Jiter::new(json_data.as_bytes(), true);
    assert_eq!(jiter.next_object().unwrap(), Some("name"));
    assert_eq!(jiter.next_str().unwrap(), "John Doe");
    assert_eq!(jiter.next_key().unwrap(), Some("age"));
    assert_eq!(jiter.next_int().unwrap(), NumberInt::Int(43));
    assert_eq!(jiter.next_key().unwrap(), Some("phones"));
    assert_eq!(jiter.next_array().unwrap(), Some(Peak::String));
    // we know the next value is a string as we just asserted so
    assert_eq!(jiter.known_str().unwrap(), "+44 1234567");
    assert_eq!(jiter.array_step().unwrap(), Some(Peak::String));
    // same again
    assert_eq!(jiter.known_str().unwrap(), "+44 2345678");
    // next we'll get `None` from `array_step` as the array is finished
    assert_eq!(jiter.array_step().unwrap(), None);
    // and `None` from `next_key` as the object is finished
    assert_eq!(jiter.next_key().unwrap(), None);
    // and we check there's nothing else in the input
    jiter.finish().unwrap();
}
```

## Benchmarks

_There are lies, damned lies and benchmarks._

In particular, serde-json benchmarks use `serde_json::Value` which is significantly slower than deserializing
to a string.

```text
running 30 tests
test big_jiter_iter                        ... bench:   7,056,970 ns/iter (+/- 93,517)
test big_jiter_value_string                ... bench:   7,928,879 ns/iter (+/- 150,790)
test big_serde_value                       ... bench:  32,281,154 ns/iter (+/- 1,152,593)
test bigints_array_jiter_iter              ... bench:      26,579 ns/iter (+/- 833)
test bigints_array_jiter_value_string      ... bench:      32,602 ns/iter (+/- 1,901)
test bigints_array_serde_value             ... bench:     148,677 ns/iter (+/- 4,517)
test floats_array_jiter_iter               ... bench:      36,071 ns/iter (+/- 2,448)
test floats_array_jiter_value_string       ... bench:      33,926 ns/iter (+/- 25,554)
test floats_array_serde_value              ... bench:     231,632 ns/iter (+/- 15,617)
test massive_ints_array_jiter_iter         ... bench:     102,095 ns/iter (+/- 1,645)
test massive_ints_array_jiter_value_string ... bench:     108,109 ns/iter (+/- 8,396)
test massive_ints_array_serde_value        ... bench:     517,150 ns/iter (+/- 53,110)
test medium_response_jiter_iter            ... bench:           0 ns/iter (+/- 0)
test medium_response_jiter_value_string    ... bench:       8,933 ns/iter (+/- 37)
test medium_response_serde_value           ... bench:      10,074 ns/iter (+/- 454)
test pass1_jiter_iter                      ... bench:           0 ns/iter (+/- 0)
test pass1_jiter_value_string              ... bench:       5,704 ns/iter (+/- 161)
test pass1_serde_value                     ... bench:       7,153 ns/iter (+/- 33)
test pass2_jiter_iter                      ... bench:         462 ns/iter (+/- 2)
test pass2_jiter_value_string              ... bench:       1,448 ns/iter (+/- 14)
test pass2_serde_value                     ... bench:       1,385 ns/iter (+/- 13)
test string_array_jiter_iter               ... bench:       1,112 ns/iter (+/- 26)
test string_array_jiter_value_string       ... bench:       4,229 ns/iter (+/- 89)
test string_array_serde_value              ... bench:       3,650 ns/iter (+/- 23)
test true_array_jiter_iter                 ... bench:         663 ns/iter (+/- 23)
test true_array_jiter_value_string         ... bench:       1,239 ns/iter (+/- 80)
test true_array_serde_value                ... bench:       1,307 ns/iter (+/- 75)
test true_object_jiter_iter                ... bench:       3,205 ns/iter (+/- 177)
test true_object_jiter_value_string        ... bench:       5,963 ns/iter (+/- 375)
test true_object_serde_value               ... bench:       7,686 ns/iter (+/- 507)

test result: ok. 0 passed; 0 failed; 0 ignored; 30 measured

     Running benches/python.rs (target/release/deps/python-11d488ef3a08ee17)

running 4 tests
test python_parse_medium_response ... bench:       8,397 ns/iter (+/- 183)
test python_parse_numeric         ... bench:         427 ns/iter (+/- 8)
test python_parse_other           ... bench:         160 ns/iter (+/- 8)
test python_parse_true_object     ... bench:       8,817 ns/iter (+/- 102)
```
