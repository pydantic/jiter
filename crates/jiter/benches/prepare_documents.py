#!/usr/bin/env python3
"""Verify and extract the vendored JSON documents before benchmarking."""

import hashlib
import lzma
from pathlib import Path

BENCH_DIR = Path(__file__).parent
DATA_DIR = BENCH_DIR / 'data'

# (compressed SHA-256, JSON SHA-256), matching pydantic/pydantic#13879.
DOCUMENTS = {
    'twitter': (
        '73e96118c9db8cf5af4f160e21767803842247bf2400282d0ec74f63ac561c3d',
        'a08b769f32b95f426cbc3abafcec65c1a19d3eb544d4ddf320eae142c99efc5d',
    ),
    'canada': (
        'a963c043202955759c32399ba31d917e07e05a89bb897a5513f4c07e85c7b9b9',
        'f83b3b354030d5dd58740c68ac4fecef64cb730a0d12a90362a7f23077f50d78',
    ),
    'citm_catalog': (
        'ce8710fd2de08235d947a909946e79140c39450c72e8fcde93f488011929efd5',
        'a73e7a883f6ea8de113dff59702975e60119b4b58d451d518a929f31c92e2059',
    ),
}


def main() -> None:
    for name, (compressed_hash, raw_hash) in DOCUMENTS.items():
        compressed = (DATA_DIR / f'{name}.json.xz').read_bytes()
        if hashlib.sha256(compressed).hexdigest() != compressed_hash:
            raise ValueError(
                f'{name}: compressed document differs from the recorded SHA-256'
            )
        raw = lzma.decompress(compressed)
        if hashlib.sha256(raw).hexdigest() != raw_hash:
            raise ValueError(f'{name}: JSON document differs from the recorded SHA-256')
        (BENCH_DIR / f'{name}.json').write_bytes(raw)
        print(f'{name}: verified and extracted {len(raw):,} bytes')


if __name__ == '__main__':
    main()
