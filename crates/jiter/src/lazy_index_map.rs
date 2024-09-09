use std::borrow::{Borrow, Cow};
use std::fmt;
use std::hash::Hash;
use std::slice::Iter as SliceIter;
use std::sync::atomic::AtomicU16;

use ahash::RandomState;
use indexmap::IndexMap;

/// Like [IndexMap](https://docs.rs/indexmap/latest/indexmap/) but only builds the lookup map when it's needed.
#[derive(Clone)]
pub struct LazyIndexMap<K, V> {
    inner: LazyIndexMapInner<K, V>,
}

#[derive(Clone)]
enum LazyIndexMapInner<K, V> {
    Array(LazyIndexMapArray<K, V>),
    Map(IndexMap<K, V, ahash::RandomState>),
}

impl<K, V> Default for LazyIndexMap<K, V>
where
    K: Clone + fmt::Debug + Eq + Hash,
    V: fmt::Debug,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V> fmt::Debug for LazyIndexMap<K, V>
where
    K: Clone + fmt::Debug + Eq + Hash,
    V: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

// picked to be a good tradeoff after experimenting with `lazy_map_lookup` benchmark, should cover most models
const HASHMAP_THRESHOLD: usize = 16;

/// Like [IndexMap](https://docs.rs/indexmap/latest/indexmap/) but only builds the lookup map when it's needed.
impl<K, V> LazyIndexMap<K, V>
where
    K: fmt::Debug + Eq + Hash,
    V: fmt::Debug,
{
    pub fn new() -> Self {
        Self {
            inner: LazyIndexMapInner::Array(LazyIndexMapArray::new()),
        }
    }

    pub fn insert(&mut self, key: K, value: V) {
        match self {
            Self {
                inner: LazyIndexMapInner::Array(vec),
            } => {
                if let Some((key, value)) = vec.insert(key, value) {
                    // array is full, convert to map
                    let LazyIndexMapInner::Array(vec) = std::mem::replace(
                        &mut self.inner,
                        LazyIndexMapInner::Map(IndexMap::with_capacity_and_hasher(
                            HASHMAP_THRESHOLD + 1,
                            RandomState::default(),
                        )),
                    ) else {
                        unreachable!("known to be a vec");
                    };
                    let LazyIndexMapInner::Map(map) = &mut self.inner else {
                        unreachable!("just set to be a map");
                    };
                    for (k, v) in vec.into_complete_data() {
                        map.insert(k, v);
                    }
                    map.insert(key, value);
                }
            }
            Self {
                inner: LazyIndexMapInner::Map(map),
            } => {
                map.insert(key, value);
            }
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Self {
                inner: LazyIndexMapInner::Array(vec),
            } => vec.duplicates_mask()[..vec.data().len()].count_ones(),
            Self {
                inner: LazyIndexMapInner::Map(map),
            } => map.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Self {
                inner: LazyIndexMapInner::Array(vec),
            } => vec.data().is_empty(),
            Self {
                inner: LazyIndexMapInner::Map(map),
            } => map.is_empty(),
        }
    }

    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q> + PartialEq<Q>,
        Q: Hash + Eq + ?Sized,
    {
        match &self.inner {
            LazyIndexMapInner::Array(vec) => vec.get(key),
            LazyIndexMapInner::Map(map) => map.get(key),
        }
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.iter().map(|(k, _)| k)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        match &self.inner {
            LazyIndexMapInner::Array(vec) => {
                // SAFETY: data is known to be initialized up to len
                let data = vec.data();
                let mask = vec.duplicates_mask().clone();
                LazyIndexMapIter::Vec {
                    iter: data.iter(),
                    mask: mask.into_iter(),
                }
            }
            LazyIndexMapInner::Map(map) => LazyIndexMapIter::Map(map.iter()),
        }
    }
}

mod index_map_vec {
    use bitvec::order::Lsb0;
    use std::borrow::{Borrow, Cow};
    use std::hash::{DefaultHasher, Hash, Hasher};
    use std::mem::MaybeUninit;
    use std::sync::atomic::{AtomicU16, AtomicUsize, Ordering};

    use super::HASHMAP_THRESHOLD;

    pub(super) struct LazyIndexMapArray<K, V> {
        data: Box<[MaybeUninit<(K, V)>; HASHMAP_THRESHOLD]>,
        len: usize,
        last_find: AtomicUsize,
        duplicates_mask: DuplicatesMask,
    }

    type DuplicatesMask = bitvec::BitArr!(for HASHMAP_THRESHOLD, in AtomicU16);

    impl<K, V> LazyIndexMapArray<K, V> {
        pub fn new() -> Self {
            Self {
                data: boxed_uninit_array(),
                len: 0,
                last_find: AtomicUsize::new(0),
                duplicates_mask: DuplicatesMask::ZERO,
            }
        }

        pub fn data(&self) -> &[(K, V)] {
            // SAFETY: data is known to be initialized up to len
            unsafe { std::slice::from_raw_parts(self.data.as_ptr().cast::<(K, V)>(), self.len) }
        }
    }

    impl<K: Hash + Eq, V> LazyIndexMapArray<K, V> {
        /// If the vec is full, returns the key-value pair that was not inserted.
        pub fn insert(&mut self, key: K, value: V) -> Option<(K, V)> {
            if self.len >= HASHMAP_THRESHOLD {
                return Some((key, value));
            }
            self.data[self.len].write((key, value));
            self.len += 1;
            // clear cached mask
            self.duplicates_mask = DuplicatesMask::ZERO;
            None
        }

        pub fn get<Q>(&self, key: &Q) -> Option<&V>
        where
            K: Borrow<Q> + PartialEq<Q>,
            Q: Hash + Eq + ?Sized,
        {
            if self.len == 0 {
                return None;
            }

            let data = self.data();
            let mask = self.duplicates_mask();

            let first_try = self.last_find.load(Ordering::Relaxed) + 1;
            for i in first_try..first_try + data.len() {
                let index = i % data.len();
                if !mask[index] {
                    continue;
                }
                let (k, v) = &data[index];
                if k.borrow() == key {
                    self.last_find.store(index, Ordering::Relaxed);
                    return Some(v);
                }
            }
            None
        }

        pub fn into_complete_data(self) -> impl IntoIterator<Item = (K, V)> {
            self.data
                .into_iter()
                .take(self.len)
                // SAFETY: reading initialized section only
                .map(|x| unsafe { x.assume_init() })
        }

        pub fn duplicates_mask(&self) -> &DuplicatesMask {
            let data = self.data();
            if self.duplicates_mask == DuplicatesMask::ZERO {
                let new_mask = build_duplicates_mask(data);
                // FIXME: is there a way to write the whole thing at once?
                for i in 0..data.len() {
                    self.duplicates_mask.set_aliased(i, new_mask[i]);
                }
            }
            &self.duplicates_mask
        }
    }

    impl<'j> LazyIndexMapArray<Cow<'j, str>, crate::JsonValue<'j>> {
        pub fn to_static(&self) -> LazyIndexMapArray<Cow<'static, str>, crate::JsonValue<'static>> {
            let mut new_data = boxed_uninit_array();
            for (i, (k, v)) in self.data().iter().enumerate() {
                new_data[i] = MaybeUninit::new((Cow::Owned(k.to_string()), v.to_static()));
            }
            LazyIndexMapArray {
                data: new_data,
                len: self.len,
                last_find: AtomicUsize::new(self.last_find.load(Ordering::Relaxed)),
                duplicates_mask: self.duplicates_mask.clone(),
            }
        }
    }

    impl<K: Clone, V: Clone> Clone for LazyIndexMapArray<K, V> {
        fn clone(&self) -> Self {
            let mut new_data = boxed_uninit_array();
            for (i, value) in self.data().iter().enumerate() {
                // SAFETY: initialized up to i
                new_data[i] = MaybeUninit::new(value.clone());
            }
            LazyIndexMapArray {
                data: new_data,
                len: self.len,
                last_find: AtomicUsize::new(self.last_find.load(Ordering::Relaxed)),
                duplicates_mask: self.duplicates_mask.clone(),
            }
        }
    }

    fn build_duplicates_mask<K: Hash + Eq, V>(data: &[(K, V)]) -> bitvec::BitArr!(for HASHMAP_THRESHOLD, in u16) {
        let hashes_and_indices: &mut [(u64, usize)] = &mut [(0u64, 0usize); HASHMAP_THRESHOLD][..data.len()];
        let mut mask = bitvec::bitarr![u16, Lsb0; 1; HASHMAP_THRESHOLD];
        for (i, (k, _)) in data.iter().enumerate() {
            // SAFETY: data is known to be initialized
            let mut hasher = DefaultHasher::new();
            k.hash(&mut hasher);
            let hash = hasher.finish();
            hashes_and_indices[i] = (hash, i);
        }
        hashes_and_indices.sort_unstable();

        for i in 0..data.len() {
            let (hash, index) = hashes_and_indices[i];
            for (next_hash, next_index) in hashes_and_indices.iter().skip(i + 1) {
                if *next_hash != hash {
                    break;
                }
                // is a duplicate key; prefer the later element
                if data[*next_index].0 == data[index].0 {
                    mask.set(index, false);
                    break;
                }
            }
        }
        mask
    }

    // in the future this should be Box::new([const { MaybeUninit::<T>::uninit() }; 16]);
    // waiting on inline const expressions to be on stable
    fn boxed_uninit_array<T>() -> Box<[MaybeUninit<T>; 16]> {
        Box::new([
            MaybeUninit::uninit(),
            MaybeUninit::uninit(),
            MaybeUninit::uninit(),
            MaybeUninit::uninit(),
            MaybeUninit::uninit(),
            MaybeUninit::uninit(),
            MaybeUninit::uninit(),
            MaybeUninit::uninit(),
            MaybeUninit::uninit(),
            MaybeUninit::uninit(),
            MaybeUninit::uninit(),
            MaybeUninit::uninit(),
            MaybeUninit::uninit(),
            MaybeUninit::uninit(),
            MaybeUninit::uninit(),
            MaybeUninit::uninit(),
        ])
    }
}

use index_map_vec::LazyIndexMapArray;

enum LazyIndexMapIter<'a, K, V> {
    Vec {
        iter: SliceIter<'a, (K, V)>,
        // to mask duplicate entries
        mask: <bitvec::BitArr!(for HASHMAP_THRESHOLD, in AtomicU16) as IntoIterator>::IntoIter,
    },
    Map(indexmap::map::Iter<'a, K, V>),
}

impl<'a, K, V> Iterator for LazyIndexMapIter<'a, K, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            LazyIndexMapIter::Vec { iter, mask } => {
                for (k, v) in iter.by_ref() {
                    let is_not_duplicate = mask.next().expect("mask covers array length");
                    if is_not_duplicate {
                        return Some((k, v));
                    }
                }
                None
            }
            LazyIndexMapIter::Map(iter) => iter.next(),
        }
    }
}

impl<'j> LazyIndexMap<Cow<'j, str>, crate::JsonValue<'j>> {
    pub(crate) fn to_static(&self) -> LazyIndexMap<Cow<'static, str>, crate::JsonValue<'static>> {
        let inner = match &self.inner {
            LazyIndexMapInner::Array(vec) => LazyIndexMapInner::Array(vec.to_static()),
            LazyIndexMapInner::Map(map) => LazyIndexMapInner::Map(
                map.iter()
                    .map(|(k, v)| (Cow::Owned(k.to_string()), v.to_static()))
                    .collect(),
            ),
        };
        LazyIndexMap { inner }
    }
}

impl<K: PartialEq, V: PartialEq> PartialEq for LazyIndexMap<K, V>
where
    K: fmt::Debug + Eq + Hash,
    V: fmt::Debug,
{
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter())
    }
}
