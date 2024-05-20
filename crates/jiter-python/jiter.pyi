import decimal
from typing import Any, Literal

def from_json(
    json_data: bytes,
    /,
    *,
    allow_inf_nan: bool = True,
    cache_strings: Literal[True, False, "all", "keys", "none"] = "all",
    allow_partial: bool = False,
    catch_duplicate_keys: bool = False,
    lossless_floats: bool = False,
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
        lossless_floats: if True, preserve full detail on floats using `JsonFloat`

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


class JsonFloat:
    """
    Represents a float from JSON, by holding the underlying bytes representing a float from JSON.
    """
    def __init__(self, json_float: bytes):
        """Construct a JsonFloat object from a JSON bytes slice"""

    def as_float(self) -> float:
        """Construct a Python float from the JSON bytes slice"""

    def as_decimal(self) -> decimal.Decimal:
        """Construct a Python Decimal from the JSON bytes slice"""

    def as_bytes(self) -> bytes:
        """Return the JSON bytes slice as bytes"""

    def __str__(self):
        """Return the JSON bytes slice as a string"""

    def __repr__(self):
        ...
