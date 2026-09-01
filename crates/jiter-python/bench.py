import argparse
import json
import math
import timeit
from pathlib import Path

CASES = {
    'array_short_strings': '[{}]'.format(', '.join('"123"' for _ in range(100_000))),
    'object_short_strings': '{%s}'  # noqa UP031
    % ', '.join(f'"{i}": "{i}x"' for i in range(100_000)),
    'array_short_arrays': '[{}]'.format(
        ', '.join('["a", "b", "c", "d"]' for _ in range(10_000))
    ),
    'one_long_string': json.dumps('x' * 100),
    'one_short_string': b'"foobar"',
    '1m_strings': json.dumps([str(i) for i in range(1_000_000)]),
}

BENCHES_DIR = Path(__file__).parent.parent / 'jiter/benches/'

for p in BENCHES_DIR.glob('*.json'):
    CASES[p.stem] = p.read_bytes()


def run_bench(func, d: bytes, fast: bool):
    timer = timeit.Timer(
        'func(json_data)', setup='', globals={'func': func, 'json_data': d}
    )
    if fast:
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
    'orjson': setup_orjson,
    'jiter-cache': setup_jiter_cache,
    'jiter': setup_jiter,
    'ujson': setup_ujson,
    'json': setup_json,
}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--case', default='all', choices=[*CASES.keys(), 'all'])
    parser.add_argument('--fast', action='store_true', default=False)
    parser.add_argument(
        'parsers', nargs='*', default='all', choices=[*PARSERS.keys(), 'all']
    )
    args = parser.parse_args()

    parsers = [*PARSERS.keys()] if 'all' in args.parsers else args.parsers
    cases = [*CASES.keys()] if args.case == 'all' else [args.case]
    slowdowns: dict[str, list[float]] = {parser: [] for parser in parsers}

    for name in cases:
        print(f'Case: {name}')

        json_data = CASES[name]
        if isinstance(json_data, str):
            json_data = json_data.encode()
        expected = json.dumps(json.loads(json_data))
        times = []
        for parser in parsers:
            func = PARSERS[parser]()
            try:
                time = run_bench(func, json_data, args.fast)
                valid = json.dumps(func(json_data)) == expected
            except Exception:  # noqa: BLE001
                times.append((parser, None, False))
                continue
            times.append((parser, time, valid))

        times.sort(key=lambda x: (not x[2], x[1] or math.inf))
        best = times[0][1]

        print(f'{"package":>12} | {"time µs":>10} | slowdown')
        print(f'{"-" * 13}|{"-" * 12}|{"-" * 9}')
        for name, time, valid in times:
            if time is None:
                print(f'{name:>12} | {"-":>10} | {"error":>8}')
            elif valid:
                print(f'{name:>12} | {time * 1_000_000:10.2f} | {time / best:8.2f}')
                slowdowns[name].append(time / best)
            else:
                print(f'{name:>12} | {time * 1_000_000:10.2f} | {"invalid":>8}')
        print()

    if len(cases) > 1:
        print_summary(slowdowns)


def print_summary(slowdowns: dict[str, list[float]]) -> None:
    rows = []
    for parser, ratios in slowdowns.items():
        if not ratios:
            print(f'{parser}: no valid results, excluded from summary')
            continue
        geomean = math.exp(sum(map(math.log, ratios)) / len(ratios))
        rows.append((parser, geomean, max(ratios), sum(r == 1 for r in ratios)))
    if not rows:
        return
    rows.sort(key=lambda r: r[1])
    best = rows[0][1]

    print(
        'Summary (slowdown relative to the fastest package in each case, '
        'excluding cases where the package returned the wrong result):'
    )
    print(
        f'{"rank":>4} | {"package":>12} | {"geomean":>8} | {"vs best":>8} | {"worst":>8} | {"wins":>4}'
    )
    print(f'{"-" * 5}|{"-" * 14}|{"-" * 10}|{"-" * 10}|{"-" * 10}|{"-" * 5}')
    for rank, (parser, geomean, worst, wins) in enumerate(rows, start=1):
        print(
            f'{rank:>4} | {parser:>12} | {geomean:8.2f} | {geomean / best:8.2f} | {worst:8.2f} | {wins:>4}'
        )


if __name__ == '__main__':
    main()
