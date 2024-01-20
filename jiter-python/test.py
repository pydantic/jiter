import jiter_python

if __name__ == '__main__':
    assert jiter_python.from_json(b'[true, false, null, 123, 456.7]') == [True, False, None, 123, 456.7]
    assert jiter_python.from_json(b'{"foo": "bar"}') == {'foo': 'bar'}
