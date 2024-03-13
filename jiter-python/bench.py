import timeit
from pathlib import Path

import orjson
import jiter_python
import ujson
import json

cases = [
    ('medium_response', Path('../benches/medium_response.json').read_bytes()),
    ('massive_ints_array', Path('../benches/massive_ints_array.json').read_bytes()),
    ('array_short_strings', '[{}]'.format(', '.join('"123"' for _ in range(100_000)))),
    ('object_short_strings', '{%s}' % ', '.join(f'"{i}": "{i}x"' for i in range(100_000))),
    ('array_short_arrays', '[{}]'.format(', '.join('["a", "b", "c", "d"]' for _ in range(10_000)))),
    ('one_long_string', json.dumps('x' * 100)),
    ('one_short_string', b'"foobar"'),
    ('1m_strings', json.dumps([str(i) for i in range(1_000_000)])),
]


def run_bench(func, d):
    if isinstance(d, str):
        d = d.encode()
    timer = timeit.Timer(
        'func(json_data)', setup='', globals={'func': func, 'json_data': d}
    )
    n, t = timer.autorange()
    iter_time = t / n
    # print(f'{func.__module__}.{func.__name__}', iter_time)
    return iter_time


for name, json_data in cases:
    print(f'Case: {name}')
    times = [
        ('orjson', run_bench(lambda d: orjson.loads(d), json_data)),
        ('jiter-cache', run_bench(lambda d: jiter_python.from_json(d), json_data)),
        ('jiter', run_bench(lambda d: jiter_python.from_json(d, cache_strings=False), json_data)),
        ('ujson', run_bench(lambda d: ujson.loads(d), json_data)),
        ('json', run_bench(lambda d: json.loads(d), json_data)),
    ]

    times.sort(key=lambda x: x[1])
    best = times[0][1]

    print(f'{"package":>12} | {"time µs":>10} | slowdown')
    print(f'{"-" * 13}|{"-" * 12}|{"-" * 9}')
    for name, time in times:
        print(f'{name:>12} | {time * 1_000_000:10.2f} | {time / best:8.2f}')
    print('')
