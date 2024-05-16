from typing import Any, Literal

def from_json(
    data: bytes,
    *,
    allow_inf_nan: bool = True,
    cache_strings: Literal[True, False, "all", "keys", "none"] = True,
    allow_partial: bool = False,
    catch_duplicate_keys: bool = False,
) -> Any: ...
def cache_clear() -> None: ...
def cache_usage() -> int: ...
