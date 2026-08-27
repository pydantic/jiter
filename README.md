# jiter

[![CI](https://github.com/pydantic/jiter/actions/workflows/ci.yml/badge.svg?event=push)](https://github.com/pydantic/jiter/actions/workflows/ci.yml?query=branch%3Amain)
[![Crates.io](https://img.shields.io/crates/v/jiter?color=green)](https://crates.io/crates/jiter)
[![CodSpeed Badge](https://img.shields.io/endpoint?url=https://codspeed.io/badge.json)](https://codspeed.io/pydantic/jiter)

Fast iterable JSON parser.

Documentation is available at [docs.rs/jiter](https://docs.rs/jiter).

jiter has three interfaces:
* `JsonValue` an enum representing JSON data
* `Jiter` an iterator over JSON data
* `PythonParse` which parses a JSON string into a Python object

## JsonValue Example

See [the `JsonValue` docs](https://docs.rs/jiter/latest/jiter/enum.JsonValue.html) for more details.

```rust
use jiter::JsonValue;

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

To use [Jiter](https://docs.rs/jiter/latest/jiter/struct.Jiter.html), you need to know what schema you're expecting:

```rust
use jiter::{Jiter, NumberInt, Peek};

let json_data = r#"
    {
        "name": "John Doe",
        "age": 43,
        "phones": [
            "+44 1234567",
            "+44 2345678"
        ]
    }"#;
let mut jiter = Jiter::new(json_data.as_bytes());
assert_eq!(jiter.next_object().unwrap(), Some("name"));
assert_eq!(jiter.next_str().unwrap(), "John Doe");
assert_eq!(jiter.next_key().unwrap(), Some("age"));
assert_eq!(jiter.next_int().unwrap(), NumberInt::Int(43));
assert_eq!(jiter.next_key().unwrap(), Some("phones"));
assert_eq!(jiter.next_array().unwrap(), Some(Peek::String));
// we know the next value is a string as we just asserted so
assert_eq!(jiter.known_str().unwrap(), "+44 1234567");
assert_eq!(jiter.array_step().unwrap(), Some(Peek::String));
// same again
assert_eq!(jiter.known_str().unwrap(), "+44 2345678");
// next we'll get `None` from `array_step` as the array is finished
assert_eq!(jiter.array_step().unwrap(), None);
// and `None` from `next_key` as the object is finished
assert_eq!(jiter.next_key().unwrap(), None);
// and we check there's nothing else in the input
jiter.finish().unwrap();
```

## Benchmarks

_There are lies, damned lies and benchmarks._

In particular, serde-json benchmarks use `serde_json::Value` which is significantly slower than deserializing
to a string.

For more details, see [the benchmarks](https://github.com/pydantic/jiter/tree/main/crates/jiter/benches).

| benchmark | `jiter` iter | `jiter` value | `serde` value | `serde`/`jiter` |
| --- | ---: | ---: | ---: | ---: |
| **strings** | | | | |
| x100 | 10ns | 11ns | 37ns | 3.2x |
| sentence | 234ns | 278ns | 296ns | 1.1x |
| unicode | 291ns | 307ns | 322ns | 1.1x |
| unicode_dense | 150ns | 152ns | 177ns | 1.2x |
| string_array | 470ns | 954ns | 3.0µs | 3.2x |
| json_cases_strings | - | 18.85ms | 59.62ms | 3.2x |
| json_cases_escapes | - | 1.70ms | 2.52ms | 1.5x |
| json_cases_non-ascii | - | 830.0µs | 1.16ms | 1.4x |
| **numbers** | | | | |
| short_numbers | - | 9.1µs | 37.6µs | 4.1x |
| floats_array | 15.6µs | 23.9µs | 117.9µs | 4.9x |
| bigints_array | 10.4µs | 16.4µs | 69.4µs | 4.2x |
| massive_ints_array | 74.1µs | 79.2µs | 279.9µs | 3.5x |
| big | 3.04ms | 4.37ms | 20.73ms | 4.7x |
| json_cases_numbers | - | 11.29ms | 54.53ms | 4.8x |
| json_cases_ints | - | 11.13ms | 50.96ms | 4.6x |
| json_cases_floats | - | 3.43ms | 15.90ms | 4.6x |
| **constants** | | | | |
| true_array | 196ns | 637ns | 1.1µs | 1.7x |
| true_object | 2.2µs | 1.5µs | 5.7µs | 3.8x |
| json_cases_constants | - | 27.4µs | 58.7µs | 2.1x |
| **documents** | | | | |
| pass1 | - | 2.2µs | 5.4µs | 2.5x |
| pass2 | 314ns | 759ns | 574ns | 0.8x |
| medium_response | - | 2.4µs | 6.9µs | 2.9x |
| json_cases_all | - | 26.74ms | 90.50ms | 3.4x |
| json_cases_arrays | - | 11.13ms | 47.00ms | 4.2x |
| json_cases_objects | - | 17.17ms | 53.85ms | 3.1x |
| json_cases_deep | - | 5.81ms | 12.97ms | 2.2x |
| json_cases_whitespace | - | 6.90ms | 16.51ms | 2.4x |
| json_cases_error | - | 2.73ms | 11.91ms | 4.4x |

## Part of the Pydantic Stack

The Pydantic Stack is everything you need to ship production-grade AI agents:

- [Pydantic AI](https://pydantic.dev/pydantic-ai?utm_source=github&utm_medium=readme&utm_campaign=jiter) - Type-safe agent framework
- [Pydantic Logfire](https://pydantic.dev/logfire?utm_source=github&utm_medium=readme&utm_campaign=jiter) - AI-first, full-stack observability
- [Logfire AI Gateway](https://pydantic.dev/ai-gateway?utm_source=github&utm_medium=readme&utm_campaign=jiter) - Unified LLM proxy
