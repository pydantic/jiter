from typing import Any, Literal

def from_json(
    json_data: bytes,
    /,
    *,
    allow_inf_nan: bool = True,
    cache_strings: Literal[True, False, "all", "keys", "none"] = True,
    allow_partial: Literal[True, False, "off", "on", "trailing-strings"] = False,
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
        allow_partial: How to handle incomplete strings:
            - False / 'off' - raise an exception if the input is incomplete
            - True / 'on' - allow incomplete JSON but discard the last string if it is incomplete
            - 'trailing-strings' - allow incomplete JSON, and include the last incomplete string in the output
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
