# batson

Binary Alternative To (J)SON. Designed to be very fast to query.

Inspired by Postgres' [JSONB type](https://github.com/postgres/postgres/commit/d9134d0a355cfa447adc80db4505d5931084278a?diff=unified&w=0) and Snowflake's [VARIANT type](https://www.youtube.com/watch?v=jtjOfggD4YY).

For a relatively small JSON document (3KB), batson is 14 to 126x faster than Jiter, and 106 to 588x faster than Serde.

```
test medium_get_str_found_batson   ... bench:          51 ns/iter (+/- 1)
test medium_get_str_found_jiter    ... bench:         755 ns/iter (+/- 66)
test medium_get_str_found_serde    ... bench:       5,420 ns/iter (+/- 93)
test medium_get_str_missing_batson ... bench:           9 ns/iter (+/- 0)
test medium_get_str_missing_jiter  ... bench:       1,135 ns/iter (+/- 46)
test medium_get_str_missing_serde  ... bench:       5,292 ns/iter (+/- 324)
```
