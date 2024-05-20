from decimal import Decimal

import jiter
import pytest
from math import inf
from dirty_equals import IsFloatNan


def test_python_parse_numeric():
    parsed = jiter.from_json(
        b'  { "int": 1, "bigint": 123456789012345678901234567890, "float": 1.2}  '
    )
    assert parsed == {"int": 1, "bigint": 123456789012345678901234567890, "float": 1.2}


def test_python_parse_other_cached():
    parsed = jiter.from_json(
        b'["string", true, false, null, NaN, Infinity, -Infinity]',
        allow_inf_nan=True,
        cache_strings=True,
    )
    assert parsed == ["string", True, False, None, IsFloatNan(), inf, -inf]


def test_python_parse_other_no_cache():
    parsed = jiter.from_json(
        b'["string", true, false, null]',
        cache_strings=False,
    )
    assert parsed == ["string", True, False, None]


def test_python_disallow_nan():
    with pytest.raises(ValueError, match="expected value at line 1 column 2"):
        jiter.from_json(b"[NaN]", allow_inf_nan=False)


def test_error():
    with pytest.raises(ValueError, match="EOF while parsing a list at line 1 column 9"):
        jiter.from_json(b'["string"')


def test_recursion_limit():
    with pytest.raises(
        ValueError, match="recursion limit exceeded at line 1 column 202"
    ):
        jiter.from_json(b"[" * 10_000)


def test_recursion_limit_incr():
    json = b"[" + b", ".join(b"[1]" for _ in range(2000)) + b"]"
    v = jiter.from_json(json)
    assert len(v) == 2000

    v = jiter.from_json(json)
    assert len(v) == 2000


def test_extracted_value_error():
    with pytest.raises(ValueError, match="expected value at line 1 column 1"):
        jiter.from_json(b"xx")


def test_partial_array():
    json = b'["string", true, null, 1, "foo'
    parsed = jiter.from_json(json, allow_partial=True)
    assert parsed == ["string", True, None, 1]

    # test that stopping at every points is ok
    for i in range(1, len(json)):
        parsed = jiter.from_json(json[:i], allow_partial=True)
        assert isinstance(parsed, list)


def test_partial_array_first():
    json = b"["
    parsed = jiter.from_json(json, allow_partial=True)
    assert parsed == []

    with pytest.raises(ValueError, match="EOF while parsing a list at line 1 column 1"):
        jiter.from_json(json)


def test_partial_object():
    json = b'{"a": 1, "b": 2, "c'
    parsed = jiter.from_json(json, allow_partial=True)
    assert parsed == {"a": 1, "b": 2}

    # test that stopping at every points is ok
    for i in range(1, len(json)):
        parsed = jiter.from_json(json, allow_partial=True)
        assert isinstance(parsed, dict)


def test_partial_nested():
    json = b'{"a": 1, "b": 2, "c": [1, 2, {"d": 1, '
    parsed = jiter.from_json(json, allow_partial=True)
    assert parsed == {"a": 1, "b": 2, "c": [1, 2, {"d": 1}]}

    # test that stopping at every points is ok
    for i in range(1, len(json)):
        parsed = jiter.from_json(json[:i], allow_partial=True)
        assert isinstance(parsed, dict)


def test_python_cache_usage_all():
    jiter.cache_clear()
    parsed = jiter.from_json(b'{"foo": "bar", "spam": 3}', cache_strings="all")
    assert parsed == {"foo": "bar", "spam": 3}
    assert jiter.cache_usage() == 3


def test_python_cache_usage_keys():
    jiter.cache_clear()
    parsed = jiter.from_json(b'{"foo": "bar", "spam": 3}', cache_strings="keys")
    assert parsed == {"foo": "bar", "spam": 3}
    assert jiter.cache_usage() == 2


def test_python_cache_usage_none():
    jiter.cache_clear()
    parsed = jiter.from_json(
        b'{"foo": "bar", "spam": 3}',
        cache_strings="none",
    )
    assert parsed == {"foo": "bar", "spam": 3}
    assert jiter.cache_usage() == 0


def test_use_tape():
    json = '  "foo\\nbar"  '.encode()
    jiter.cache_clear()
    parsed = jiter.from_json(json, cache_strings=False)
    assert parsed == "foo\nbar"


def test_unicode():
    json = '{"💩": "£"}'.encode()
    jiter.cache_clear()
    parsed = jiter.from_json(json, cache_strings=False)
    assert parsed == {"💩": "£"}


def test_unicode_cache():
    json = '{"💩": "£"}'.encode()
    jiter.cache_clear()
    parsed = jiter.from_json(json)
    assert parsed == {"💩": "£"}


def test_json_float():
    f = jiter.JsonFloat('123.45')
    assert str(f) == '123.45'
    assert repr(f) == 'JsonFloat(123.45)'
    assert f.as_float() == 123.45
    assert f.as_decimal() == Decimal('123.45')


def test_json_float_scientific():
    f = jiter.JsonFloat('123e4')
    assert str(f) == '123e4'
    assert f.as_float() == 123e4
    assert f.as_decimal() == Decimal('123e4')


def test_json_float_invalid():
    with pytest.raises(ValueError, match='trailing characters at line 1 column 6'):
        jiter.JsonFloat('123.4x')
