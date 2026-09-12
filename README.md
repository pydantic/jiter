# jiter

[![CI](https://github.com/pydantic/jiter/actions/workflows/ci.yml/badge.svg?event=push)](https://github.com/pydantic/jiter/actions/workflows/ci.yml?query=branch%3Amain)
[![Crates.io](https://img.shields.io/crates/v/jiter?color=green)](https://crates.io/crates/jiter)
[![CodSpeed Badge](https://img.shields.io/endpoint?url=https://codspeed.io/badge.json)](https://codspeed.io/pydantic/jiter)

Fast iterable JSON parser.

Documentation is available at [docs.rs/jiter](https://docs.rs/jiter).

jiter has three interfaces:

- `JsonValue` an enum representing JSON data
- `Jiter` an iterator over JSON data
- `PythonParse` which parses a JSON string into a Python object

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

| benchmark               | `jiter` iter | `jiter` value | `serde` value | `serde`/`jiter` |
| ----------------------- | -----------: | ------------: | ------------: | --------------: |
| **strings**             |              |               |               |                 |
| x100                    |          8ns |           9ns |          40ns |            4.4x |
| sentence                |        232ns |         124ns |         293ns |            2.4x |
| unicode                 |        268ns |         159ns |         308ns |            1.9x |
| unicode_dense           |        152ns |         152ns |         177ns |            1.2x |
| string_array            |        466ns |         751ns |         3.0µs |            4.0x |
| pass2                   |        304ns |         785ns |         576ns |            0.7x |
| json_cases_strings      |            - |       14.92ms |       58.85ms |            3.9x |
| json_cases_escapes      |            - |        1.62ms |        2.44ms |            1.5x |
| json_cases_non-ascii    |            - |       715.5µs |       995.3µs |            1.4x |
| **numbers**             |              |               |               |                 |
| short_numbers           |            - |        10.9µs |        37.6µs |            3.4x |
| floats_array            |       15.2µs |        19.2µs |       117.2µs |            6.1x |
| doubles_array           |       13.2µs |        17.6µs |       110.9µs |            6.3x |
| short_floats            |        9.7µs |        12.6µs |        46.6µs |            3.7x |
| long_significand_floats |       12.1µs |        13.9µs |        94.7µs |            6.8x |
| bigints_array           |       12.2µs |        12.5µs |        69.3µs |            5.6x |
| massive_ints_array      |       73.2µs |        77.9µs |       284.9µs |            3.7x |
| big                     |       2.93ms |        3.96ms |       20.32ms |            5.1x |
| json_cases_numbers      |            - |        7.81ms |       50.42ms |            6.5x |
| json_cases_ints         |            - |        7.82ms |       50.27ms |            6.4x |
| json_cases_floats       |            - |        3.04ms |       17.68ms |            5.8x |
| **constants**           |              |               |               |                 |
| true_array              |        194ns |         477ns |         1.1µs |            2.3x |
| true_object             |        2.4µs |         1.4µs |         6.0µs |            4.2x |
| json_cases_constants    |            - |        25.6µs |        66.3µs |            2.6x |
| **documents**           |              |               |               |                 |
| pass1                   |            - |         1.8µs |         5.3µs |            3.0x |
| medium_response         |            - |         2.2µs |         6.8µs |            3.1x |
| json_cases_all          |            - |       22.15ms |       90.38ms |            4.1x |
| json_cases_arrays       |            - |       10.78ms |       45.96ms |            4.3x |
| json_cases_objects      |            - |       12.98ms |       53.66ms |            4.1x |
| json_cases_deep         |            - |        4.63ms |       12.83ms |            2.8x |
| json_cases_whitespace   |            - |        6.24ms |       16.58ms |            2.7x |
| json_cases_error        |            - |        2.38ms |       11.87ms |            5.0x |

## Part of the Pydantic Stack

The Pydantic Stack is everything you need to ship production-grade AI agents:

- [Pydantic AI](https://pydantic.dev/pydantic-ai?utm_source=github&utm_medium=readme&utm_campaign=jiter) - Type-safe agent framework
- [Pydantic Logfire](https://pydantic.dev/logfire?utm_source=github&utm_medium=readme&utm_campaign=jiter) - AI-first, full-stack observability
- [Logfire AI Gateway](https://pydantic.dev/ai-gateway?utm_source=github&utm_medium=readme&utm_campaign=jiter) - Unified LLM proxy
