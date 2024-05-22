import argparse
import os
import timeit
from pathlib import Path

import json

FAST = bool(os.getenv('FAST'))
THIS_DIR = Path(__file__).parent

cases = [
    ("medium_response", (THIS_DIR / "../jiter/benches/medium_response.json").read_bytes()),
    (
        "massive_ints_array",
        (THIS_DIR / "../jiter/benches/massive_ints_array.json").read_bytes(),
    ),
    ("array_short_strings", "[{}]".format(", ".join('"123"' for _ in range(100_000)))),
    (
        "object_short_strings",
        "{%s}" % ", ".join(f'"{i}": "{i}x"' for i in range(100_000)),
    ),
    (
        "array_short_arrays",
        "[{}]".format(", ".join('["a", "b", "c", "d"]' for _ in range(10_000))),
    ),
    ("one_long_string", json.dumps("x" * 100)),
    ("one_short_string", b'"foobar"'),
    ("1m_strings", json.dumps([str(i) for i in range(1_000_000)])),
]


def run_bench(func, d):
    if isinstance(d, str):
        d = d.encode()
    timer = timeit.Timer(
        "func(json_data)", setup="", globals={"func": func, "json_data": d}
    )
    if FAST:
        return timer.timeit(1)
    else:
        n, t = timer.autorange()
        iter_time = t / n
        # print(f'{func.__module__}.{func.__name__}', iter_time)
        return iter_time


def setup_orjson():
    import orjson

    return lambda data: orjson.loads(data)


def setup_jiter_cache():
    import jiter

    return lambda data: jiter.from_json(data, cache_mode=True)


def setup_jiter():
    import jiter

    return lambda data: jiter.from_json(data, cache_mode=False)


def setup_ujson():
    import ujson

    return lambda data: ujson.loads(data)


def setup_json():
    import json

    return lambda data: json.loads(data)


PARSERS = {
    "orjson": setup_orjson,
    "jiter-cache": setup_jiter_cache,
    "jiter": setup_jiter,
    "ujson": setup_ujson,
    "json": setup_json,
}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "parsers", nargs="*", default="all", choices=[*PARSERS.keys(), "all"]
    )
    args = parser.parse_args()

    if "all" in args.parsers:
        args.parsers = [*PARSERS.keys()]

    for name, json_data in cases:
        print(f"Case: {name}")

        times = [
            (parser, run_bench(PARSERS[parser](), json_data)) for parser in args.parsers
        ]

        times.sort(key=lambda x: x[1])
        best = times[0][1]

        print(f'{"package":>12} | {"time µs":>10} | slowdown')
        print(f'{"-" * 13}|{"-" * 12}|{"-" * 9}')
        for name, time in times:
            print(f"{name:>12} | {time * 1_000_000:10.2f} | {time / best:8.2f}")
        print("")


if __name__ == "__main__":
    main()
