# Vendored JSON benchmark documents

These documents come from [nativejson-benchmark](https://github.com/miloyip/nativejson-benchmark/tree/master/data):

- `twitter.json`: API response with multilingual text
- `canada.json`: GeoJSON with roughly 111,000 floating-point coordinates
- `citm_catalog.json`: nested concert-ticketing catalogue

The same compressed files are used by [Pydantic's document-shape benchmarks](https://github.com/pydantic/pydantic/pull/13879). The source link is above; `../prepare_documents.py` records the SHA-256 hashes of both the compressed and decompressed bytes. Benchmark setup runs it automatically to verify and extract the documents into `benches/`; the extracted files are ignored by Git.

## License

The MIT License (MIT)

Copyright (c) 2014 Milo Yip

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
