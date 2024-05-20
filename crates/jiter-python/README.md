# jiter

[![CI](https://github.com/pydantic/jiter/workflows/CI/badge.svg?event=push)](https://github.com/pydantic/jiter/actions?query=event%3Apush+branch%3Amain+workflow%3ACI)
[![pypi](https://img.shields.io/pypi/v/jiter.svg)](https://pypi.python.org/pypi/jiter)
[![versions](https://img.shields.io/pypi/pyversions/jiter.svg)](https://github.com/pydantic/jiter)
[![license](https://img.shields.io/github/license/pydantic/jiter.svg)](https://github.com/pydantic/jiter/blob/main/LICENSE)

This is a standalone version of the JSON parser used in `pydantic-core`. The recommendation is to only use this package directly if you do not use `pydantic`.

The API is extremely minimal:

```python
def from_json(
    json_data: bytes,
    /,
    *,
    allow_inf_nan: bool = True,
    cache_strings: Literal[True, False, "all", "keys", "none"] = True,
    allow_partial: bool = False,
    catch_duplicate_keys: bool = False,
) -> Any:
    """
    Parse input bytes into a JSON object.

    Arguments:
        json_data: The JSON data to parse
        allow_inf_nan: Whether to allow infinity (`Infinity` an `-Infinity`) and `NaN` values to float fields.
            Defaults to True.
        cache_strings: cache Python strings to improve performance at the cost of some memory usage
            - True / 'all' - cache all strings
            - 'keys' - cache only object keys
            - False / 'none' - cache nothing
        allow_partial: if True, return parsed content when reaching EOF without closing objects and arrays
        catch_duplicate_keys: if True, raise an exception if objects contain the same key multiple times

    Returns:
        Python object built from the JSON input.
    """

def cache_clear() -> None:
    """
    Reset the string cache.
    """

def cache_usage() -> int:
    """
    get the size of the string cache.

    Returns:
        Size of the string cache in bytes.
    """
```
