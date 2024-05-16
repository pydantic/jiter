# jiter

This is a standalone version of the JSON parser used in `pydantic-core`. The recommendation is to only use this package directly if you do not use `pydantic`.

The API is extremely minimal:

```python
def from_json(
    data: bytes,
    *,
    allow_inf_nan: bool = True,
    cache_strings: Literal[True, False, 'all', 'keys', 'none'] = True,
    allow_partial: bool = False,
    catch_duplicate_keys: bool = False,
) -> Any:
    """
    Parse input bytes into a JSON string.

    allow_inf_nan: if True, to allow Infinity and NaN as values in the JSON
    cache_strings: cache Python strings to improve performance at the cost of some memory usage
        - True / 'all' - cache all strings
        - 'keys' - cache only object keys
        - 'none' - cache nothing
    allow_partial: if True, return parsed content when reaching EOF without closing objects and arrays
    catch_duplicate_keys: if True, raise an exception if objects contain the same key multiple times
    """
    ...

def cache_clear() -> None:
    """Clear the string cache"""
    ...

def cache_usage() -> int:
    """Get number of strings in the cache"""
    ...
```
