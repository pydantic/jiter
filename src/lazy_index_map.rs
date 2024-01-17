use std::borrow::Borrow;
use std::cmp::{Eq, PartialEq};
use std::fmt;
use std::hash::Hash;
use std::slice::Iter as SliceIter;
use std::sync::{Arc, Mutex, OnceLock};

use ahash::AHashMap;
use smallvec::SmallVec;

/// Like [IndexMap](https://docs.rs/indexmap/latest/indexmap/) but only builds the lookup map when it's needed.
#[derive(Clone, Default)]
pub struct LazyIndexMap<K, V> {
    vec: SmallVec<[(K, V); 8]>,
    map: OnceLock<AHashMap<K, usize>>,
    last_find: Arc<Mutex<usize>>,
}

impl<K, V> fmt::Debug for LazyIndexMap<K, V>
where
    K: Clone + fmt::Debug + Eq + Hash,
    V: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter_unique()).finish()
    }
}

// picked to be a good tradeoff after experimenting with `lazy_map_lookup` benchmark, should cover most models
const HASHMAP_THRESHOLD: usize = 16;

/// Like [IndexMap](https://docs.rs/indexmap/latest/indexmap/) but only builds the lookup map when it's needed.
impl<K, V> LazyIndexMap<K, V>
where
    K: Clone + fmt::Debug + Eq + Hash,
    V: fmt::Debug,
{
    pub fn new() -> Self {
        Self {
            vec: SmallVec::new(),
            map: OnceLock::new(),
            last_find: Arc::new(Mutex::new(0)),
        }
    }

    pub fn insert(&mut self, key: K, value: V) {
        if let Some(map) = self.map.get_mut() {
            map.insert(key.clone(), self.vec.len());
        }
        self.vec.push((key, value));
    }

    pub fn len(&self) -> usize {
        self.get_map().len()
    }

    pub fn is_empty(&self) -> bool {
        self.get_map().is_empty()
    }

    pub fn get<Q: ?Sized>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q> + PartialEq<Q>,
        Q: Hash + Eq,
    {
        let vec_len = self.vec.len();
        // if the vec is longer than the threshold, we use the hashmap for lookups
        if vec_len > HASHMAP_THRESHOLD {
            self.get_map().get(key).map(|&i| &self.vec[i].1)
        } else {
            // otherwise we find the value in the vec
            // we assume the most likely position for the match is at `last_find + 1`
            match self.last_find.lock() {
                Ok(mut last_find) => {
                    let first_try = *last_find + 1;
                    for i in first_try..first_try + vec_len {
                        let index = i % vec_len;
                        let (k, v) = &self.vec[index];
                        if k == key {
                            *last_find = index;
                            return Some(v);
                        }
                    }
                    None
                }
                Err(_) => None,
            }
        }
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.vec.iter().map(|(k, _)| k)
    }

    pub fn iter(&self) -> SliceIter<'_, (K, V)> {
        self.vec.iter()
    }

    pub fn iter_unique(&self) -> impl Iterator<Item = (&K, &V)> {
        IterUnique {
            vec: &self.vec,
            map: self.get_map(),
            index: 0,
        }
    }

    fn get_map(&self) -> &AHashMap<K, usize> {
        self.map.get_or_init(|| {
            self.vec
                .iter()
                .enumerate()
                .map(|(index, (key, _))| (key.clone(), index))
                .collect()
        })
    }
}

impl<K: PartialEq, V: PartialEq> PartialEq for LazyIndexMap<K, V> {
    fn eq(&self, other: &Self) -> bool {
        self.vec == other.vec
    }
}

struct IterUnique<'a, K, V> {
    vec: &'a SmallVec<[(K, V); 8]>,
    map: &'a AHashMap<K, usize>,
    index: usize,
}

impl<'a, K: Hash + Eq, V> Iterator for IterUnique<'a, K, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        while self.index < self.vec.len() {
            let (k, v) = &self.vec[self.index];
            if let Some(map_index) = self.map.get(k) {
                if *map_index == self.index {
                    self.index += 1;
                    return Some((k, v));
                }
            }
            self.index += 1;
        }
        None
    }
}
