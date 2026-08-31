#!/usr/bin/env python3
"""Convert criterion benchmark results into a markdown table for the README.

Reads estimates straight from criterion's data directory (`target/criterion`). criterion never
removes results, so a directory that has seen more than one revision of the benchmarks still holds
the ones since renamed or deleted, and they would get rows in the table as though they still ran.
`make bench-table` clears the directory before benchmarking for that reason; run it rather than
calling this directly, and paste the printed table into the README.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

# fixture -> group, also defines row order within each group. The `json_cases_*` fixtures are the
# corpus sliced by tag (see `json_cases.rs`), so each one joins the group its tag belongs to rather
# than forming a corpus group of its own; `json_cases_all` is the whole corpus, so it sits with the
# other whole-document benchmarks.
GROUPS: list[tuple[str, list[str]]] = [
    (
        'strings',
        [
            'x100',
            'sentence',
            'unicode',
            'unicode_dense',
            'string_array',
            'pass2',
            'json_cases_strings',
            'json_cases_escapes',
            'json_cases_non-ascii',
        ],
    ),
    (
        'numbers',
        [
            'short_numbers',
            'bigints_array',
            'massive_ints_array',
            # despite the name, big.json is 1000 arrays of ints and floats with no strings,
            # objects or constants in it at all, see generate_big.py
            'big',
            'json_cases_numbers',
            'json_cases_ints',
        ],
    ),
    (
        'floats',
        [
            'floats_array',
            'short_floats',
            'doubles_array',
            'long_significand_floats',
            'exponent_floats',
            'json_cases_floats',
        ],
    ),
    ('constants', ['true_array', 'true_object', 'json_cases_constants']),
    (
        'documents',
        [
            'pass1',
            'medium_response',
            'json_cases_all',
            'json_cases_arrays',
            'json_cases_objects',
            'json_cases_deep',
            'json_cases_whitespace',
            'json_cases_error',
        ],
    ),
]

# ordered longest-first so e.g. `_jiter_value_owned` wins over `_jiter_value`
SUFFIXES = [
    'jiter_value_owned',
    'jiter_value',
    'jiter_iter',
    'jiter_skip',
    'serde_value',
    'serde_iter',
]


def parse_name(name: str) -> tuple[str, str] | None:
    for suffix in SUFFIXES:
        fixture = name.removesuffix(f'_{suffix}')
        if fixture != name:
            return fixture, suffix
    return None


def load_results(criterion_dir: Path) -> dict[str, dict[str, float]]:
    """Return {fixture: {suffix: nanoseconds}}."""
    results: dict[str, dict[str, float]] = {}
    for estimates_path in sorted(criterion_dir.glob('*/new/estimates.json')):
        parsed = parse_name(estimates_path.parent.parent.name)
        if parsed is None:
            continue
        fixture, suffix = parsed
        estimates = json.loads(estimates_path.read_text())
        estimate = estimates.get('slope') or estimates['mean']
        results.setdefault(fixture, {})[suffix] = estimate['point_estimate']
    return results


def format_time(ns: float | None) -> str:
    if ns is None:
        return '-'
    if ns < 1_000:
        return f'{ns:.0f}ns'
    if ns < 1_000_000:
        return f'{ns / 1_000:.1f}µs'
    return f'{ns / 1_000_000:.2f}ms'


def format_ratio(jiter_ns: float | None, serde_ns: float | None) -> str:
    if jiter_ns is None or serde_ns is None:
        return '-'
    return f'{serde_ns / jiter_ns:.1f}x'


def build_table(results: dict[str, dict[str, float]]) -> str:
    grouped = [
        (group, [f for f in fixtures if f in results]) for group, fixtures in GROUPS
    ]
    known = {fixture for _, fixtures in grouped for fixture in fixtures}
    other = sorted(set(results) - known)
    if other:
        grouped.append(('other', other))

    header = [
        'benchmark',
        '`jiter` iter',
        '`jiter` value',
        '`serde` value',
        '`serde`/`jiter`',
    ]
    body: list[list[str]] = []
    for group, fixtures in grouped:
        rows = []
        for fixture in fixtures:
            times = results[fixture]
            jiter_value = times.get('jiter_value')
            serde_value = times.get('serde_value')
            if serde_value is None:
                # a row without the serde comparison isn't adding anything
                continue
            rows.append(
                [
                    fixture,
                    format_time(times.get('jiter_iter')),
                    format_time(jiter_value),
                    format_time(serde_value),
                    format_ratio(jiter_value, serde_value),
                ]
            )
        if rows:
            body.append([f'**{group}**', '', '', '', ''])
            body.extend(rows)

    # pad the cells so the pipes line up in the source; the first column is left-aligned to
    # match its `---` marker, the rest right-aligned to match `---:`
    widths = [max(len(row[i]) for row in [header, *body]) for i in range(len(header))]

    def format_row(cells: list[str]) -> str:
        padded = [cells[0].ljust(widths[0])]
        padded += [cell.rjust(width) for cell, width in zip(cells[1:], widths[1:])]
        return '| ' + ' | '.join(padded) + ' |'

    separator = (
        '| '
        + ' | '.join(
            ['-' * widths[0]] + ['-' * (width - 1) + ':' for width in widths[1:]]
        )
        + ' |'
    )
    return '\n'.join(
        [format_row(header), separator] + [format_row(row) for row in body]
    )


def default_criterion_dir() -> Path:
    # repo root is three levels up from this file (crates/jiter/benches)
    for base in [Path.cwd(), Path(__file__).resolve().parents[3]]:
        candidate = base / 'target' / 'criterion'
        if candidate.is_dir():
            return candidate
    sys.exit(
        'could not find target/criterion - run `cargo bench` first or pass the path explicitly'
    )


def main() -> None:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument(
        'criterion_dir', nargs='?', type=Path, help='path to target/criterion'
    )
    args = parser.parse_args()

    criterion_dir = args.criterion_dir or default_criterion_dir()
    results = load_results(criterion_dir)
    if not results:
        sys.exit(f'no benchmark results found in {criterion_dir}')
    print(build_table(results))


if __name__ == '__main__':
    main()
