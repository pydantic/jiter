use std::sync::{Mutex, MutexGuard, PoisonError};

use ahash::random_state::RandomState;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyString};
use smallvec::SmallVec;

use crate::string_decoder::StringOutput;

#[derive(Debug, Clone, Copy, Default)]
pub enum StringCacheMode {
    #[default]
    All,
    Keys,
    None,
}

impl<'py> FromPyObject<'_, 'py> for StringCacheMode {
    type Error = PyErr;

    fn extract(ob: Borrowed<'_, 'py, PyAny>) -> PyResult<StringCacheMode> {
        if let Ok(bool_mode) = ob.cast::<PyBool>() {
            Ok(bool_mode.is_true().into())
        } else if let Ok(str_mode) = ob.extract::<&str>() {
            match str_mode {
                "all" => Ok(Self::All),
                "keys" => Ok(Self::Keys),
                "none" => Ok(Self::None),
                _ => Err(PyValueError::new_err(
                    "Invalid string cache mode, should be `'all'`, '`keys`', `'none`' or a `bool`",
                )),
            }
        } else {
            Err(PyTypeError::new_err(
                "Invalid string cache mode, should be `'all'`, '`keys`', `'none`' or a `bool`",
            ))
        }
    }
}

impl From<bool> for StringCacheMode {
    fn from(mode: bool) -> Self {
        if mode { Self::All } else { Self::None }
    }
}

/// The string cache a parse works on, taken from the pool on the first cacheable string and
/// returned to it when the parse ends.
#[derive(Default)]
pub(crate) struct StringCacheGuard(Option<PyStringCache>);

impl Drop for StringCacheGuard {
    fn drop(&mut self) {
        if let Some(cache) = self.0.take() {
            return_string_cache(cache);
        }
    }
}

pub trait StringMaybeCache {
    fn get_key<'py>(
        py: Python<'py>,
        guard: &mut StringCacheGuard,
        string_output: StringOutput<'_, '_>,
    ) -> Bound<'py, PyString>;

    fn get_value<'py>(
        py: Python<'py>,
        guard: &mut StringCacheGuard,
        string_output: StringOutput<'_, '_>,
    ) -> Bound<'py, PyString> {
        Self::get_key(py, guard, string_output)
    }
}

/// # Safety
///
/// Caller must match the ascii_only flag to the string passed in.
#[inline]
unsafe fn guarded_py_string<'py>(
    py: Python<'py>,
    guard: &mut StringCacheGuard,
    string_output: &StringOutput<'_, '_>,
) -> Bound<'py, PyString> {
    let s = string_output.as_str();
    let ascii_only = string_output.ascii_only();
    if (2..64).contains(&s.len()) {
        let cache = guard.0.get_or_insert_with(take_string_cache);
        unsafe { cache.get_or_insert(py, s, ascii_only) }
    } else {
        unsafe { pystring_fast_new_maybe_ascii(py, s, ascii_only) }
    }
}

pub struct StringCacheAll;

impl StringMaybeCache for StringCacheAll {
    fn get_key<'py>(
        py: Python<'py>,
        guard: &mut StringCacheGuard,
        string_output: StringOutput<'_, '_>,
    ) -> Bound<'py, PyString> {
        // SAFETY: `StringOutput` guarantees that its ASCII flag matches its contents.
        unsafe { guarded_py_string(py, guard, &string_output) }
    }
}

pub struct StringCacheKeys;

impl StringMaybeCache for StringCacheKeys {
    fn get_key<'py>(
        py: Python<'py>,
        guard: &mut StringCacheGuard,
        string_output: StringOutput<'_, '_>,
    ) -> Bound<'py, PyString> {
        // SAFETY: `StringOutput` guarantees that its ASCII flag matches its contents.
        unsafe { guarded_py_string(py, guard, &string_output) }
    }

    fn get_value<'py>(
        py: Python<'py>,
        _guard: &mut StringCacheGuard,
        string_output: StringOutput<'_, '_>,
    ) -> Bound<'py, PyString> {
        // SAFETY: `StringOutput` guarantees that its ASCII flag matches its contents.
        unsafe { pystring_fast_new_maybe_ascii(py, string_output.as_str(), string_output.ascii_only()) }
    }
}

pub struct StringNoCache;

impl StringMaybeCache for StringNoCache {
    fn get_key<'py>(
        py: Python<'py>,
        _guard: &mut StringCacheGuard,
        string_output: StringOutput<'_, '_>,
    ) -> Bound<'py, PyString> {
        // SAFETY: `StringOutput` guarantees that its ASCII flag matches its contents.
        unsafe { pystring_fast_new_maybe_ascii(py, string_output.as_str(), string_output.ascii_only()) }
    }
}

/// The string caches no parse is using. A parse takes one out, or builds one if there are none,
/// and puts it back when it's done, so the lock is never held while Python code can run. Under
/// the GIL parses never overlap and there is only ever one cache; on free-threaded builds the
/// pool grows to the number of parses that have overlapped, up to `MAX_POOLED_CACHES`.
static STRING_CACHE: Mutex<SmallVec<[PyStringCache; 1]>> = Mutex::new(SmallVec::new_const());

/// Each cache is a quarter of a megabyte, so a pool that grew to a large thread count would hold
/// on to that memory for good; beyond this many, a returned cache is dropped instead.
const MAX_POOLED_CACHES: usize = 8;

fn string_cache_pool() -> MutexGuard<'static, SmallVec<[PyStringCache; 1]>> {
    STRING_CACHE.lock().unwrap_or_else(PoisonError::into_inner)
}

fn take_string_cache() -> PyStringCache {
    string_cache_pool().pop().unwrap_or_default()
}

fn return_string_cache(cache: PyStringCache) {
    let mut pool = string_cache_pool();
    if pool.len() < MAX_POOLED_CACHES {
        pool.push(cache);
    }
}

/// The number of entries in the string caches no parse is using.
pub fn cache_usage() -> usize {
    string_cache_pool().iter().map(PyStringCache::usage).sum()
}

/// Clear the string caches no parse is using; a cache in use by a parse is left as it is.
pub fn cache_clear() {
    string_cache_pool().iter_mut().for_each(PyStringCache::clear);
}

/// Create a cached Python `str` from a string slice
#[inline]
pub fn cached_py_string<'py>(py: Python<'py>, s: &str) -> Bound<'py, PyString> {
    // SAFETY: the ASCII fast path is disabled.
    unsafe { cached_py_string_maybe_ascii(py, s, false) }
}

/// Create a cached Python `str` from a string slice.
///
/// # Safety
///
/// Caller must pass ascii-only string.
#[inline]
pub unsafe fn cached_py_string_ascii<'py>(py: Python<'py>, s: &str) -> Bound<'py, PyString> {
    // SAFETY: the caller guarantees that `s` is ASCII only.
    unsafe { cached_py_string_maybe_ascii(py, s, true) }
}

/// # Safety
///
/// Caller must match the ascii_only flag to the string passed in.
unsafe fn cached_py_string_maybe_ascii<'py>(py: Python<'py>, s: &str, ascii_only: bool) -> Bound<'py, PyString> {
    // SAFETY: this function's caller guarantees that `ascii_only` matches `s`.
    unsafe {
        // from tests, 0 and 1 character strings are faster not cached
        if (2..64).contains(&s.len()) {
            let mut cache = take_string_cache();
            let py_string = cache.get_or_insert(py, s, ascii_only);
            return_string_cache(cache);
            py_string
        } else {
            pystring_fast_new_maybe_ascii(py, s, ascii_only)
        }
    }
}

// Capacity should be a power of 2 so the compiler can convert `%` to a right shift below
// Using a smaller number here (e.g. 1024) seems to be faster in many cases than a larger number (like 65536)
const CAPACITY: usize = 16_384;
type Entry = Option<(u64, Py<PyString>)>;

/// This is a Fully associative cache with LRU replacement policy.
/// See https://en.wikipedia.org/wiki/Cache_placement_policies#Fully_associative_cache
#[derive(Debug)]
struct PyStringCache {
    entries: Box<[Entry; CAPACITY]>,
    hash_builder: RandomState,
}

const ARRAY_REPEAT_VALUE: Entry = None;

impl Default for PyStringCache {
    fn default() -> Self {
        Self {
            // Make sure we don't allocate a large array on the stack (e.g. using `Box::new([ARRAY_REPEAT_VALUE; CAPACITY])`)
            // to avoid potential stack overflows.
            // Note: we might want to use `Box::<[Entry; CAPACITY]>::new_zeroed()` if we ever bump the MSRV above 1.92,
            // given that `Entry` can be used as a niche optimization.
            entries: std::iter::repeat_with(|| ARRAY_REPEAT_VALUE)
                .take(CAPACITY)
                .collect::<Vec<_>>()
                .into_boxed_slice()
                .try_into()
                .unwrap(),
            hash_builder: RandomState::default(),
        }
    }
}

impl PyStringCache {
    /// Lookup the cache for an entry with the given string. If it exists, return it.
    /// If it is not set or has a different string, insert it and return it.
    ///
    /// # Safety
    ///
    /// `ascii_only` must only be set to `true` if the string is guaranteed to be ASCII only.
    unsafe fn get_or_insert<'py>(&mut self, py: Python<'py>, s: &str, ascii_only: bool) -> Bound<'py, PyString> {
        let hash = self.hash_builder.hash_one(s);

        let hash_index = hash as usize % CAPACITY;

        let set_entry = |entry: &mut Entry| {
            // SAFETY: caller upholds invariant
            let py_str = unsafe { pystring_fast_new_maybe_ascii(py, s, ascii_only) };
            if let Some((_, old_py_str)) = entry.replace((hash, py_str.clone().unbind())) {
                // micro-optimization: bind the old entry before dropping it so that PyO3 can
                // fast-path the drop (Bound::drop is faster than Py::drop)
                drop(old_py_str.into_bound(py));
            }
            py_str
        };

        // we try up to 5 contiguous slots to find a match or an empty slot
        for index in hash_index..hash_index.wrapping_add(5) {
            if let Some(entry) = self.entries.get_mut(index) {
                if let Some((entry_hash, py_str_ob)) = entry {
                    // to avoid a string comparison, we first compare the hashes
                    if *entry_hash == hash {
                        // if the hashes match, we compare the strings to be absolutely sure - as a hashmap would do
                        if py_str_ob.bind(py) == s {
                            // the strings matched, return the cached string object
                            return py_str_ob.bind(py).to_owned();
                        }
                    }
                } else {
                    // we got to an empty entry, use it
                    return set_entry(entry);
                }
            } else {
                // we reached the end of entries, break
                break;
            }
        }
        // we tried all 5 slots (or got to the end of entries) without finding a match
        // or an empty slot, make this LRU by replacing the first entry
        let entry = self.entries.get_mut(hash_index).unwrap();
        set_entry(entry)
    }

    /// get the number of entries in the cache that are set
    fn usage(&self) -> usize {
        self.entries.iter().filter(|e| e.is_some()).count()
    }

    /// clear the cache by resetting all entries to `None`
    fn clear(&mut self) {
        self.entries.fill_with(|| None);
    }
}

/// Create a new Python `str` from a string slice, with a fast path for ASCII strings
///
/// # Safety
///
/// `ascii_only` must only be set to `true` if the string is guaranteed to be ASCII only.
unsafe fn pystring_fast_new_maybe_ascii<'py>(py: Python<'py>, s: &str, ascii_only: bool) -> Bound<'py, PyString> {
    if ascii_only {
        // SAFETY: caller upholds invariant
        unsafe { pystring_ascii_new(py, s) }
    } else {
        PyString::new(py, s)
    }
}

/// Faster creation of PyString from an ASCII string, inspired by
/// <https://github.com/ijl/orjson/blob/3.10.0/src/str/create.rs#L41>
///
/// # Safety
///
/// `s` must be ASCII only
pub unsafe fn pystring_ascii_new<'py>(py: Python<'py>, s: &str) -> Bound<'py, PyString> {
    unsafe {
        #[cfg(not(any(PyPy, GraalPy, Py_LIMITED_API)))]
        {
            if s.len() <= 1 {
                return PyString::new(py, s);
            }
            // SAFETY: `PyUnicode_New` returns a new owned reference or null. Converting it to a
            // `Bound` immediately ensures an allocation failure is handled before dereferencing it.
            let py_string = Bound::from_owned_ptr(py, pyo3::ffi::PyUnicode_New(s.len() as isize, 127));
            let ptr = py_string.as_ptr();
            // see https://github.com/pydantic/jiter/pull/72#discussion_r1545485907
            debug_assert_eq!(pyo3::ffi::PyUnicode_KIND(ptr), pyo3::ffi::PyUnicode_1BYTE_KIND);
            let data_ptr = pyo3::ffi::PyUnicode_DATA(ptr).cast();
            // SAFETY: the caller guarantees that `s` is ASCII, so `PyUnicode_New` allocated a
            // writable one-byte buffer containing `s.len()` bytes followed by a null terminator.
            core::ptr::copy_nonoverlapping(s.as_ptr(), data_ptr, s.len());
            core::ptr::write(data_ptr.add(s.len()), 0);
            py_string.cast_into_unchecked()
        }

        #[cfg(any(PyPy, GraalPy, Py_LIMITED_API))]
        {
            PyString::new(py, s)
        }
    }
}
