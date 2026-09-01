//! Storage behind `Value::Map`. Tiny maps — the common record case — are a flat vec scanned
//! linearly; past `SMALL_MAX` entries they promote to a persistent HAMT whose O(1) clone makes
//! copy-then-update builtins (`set_in`, `update_in`, splats) O(log n) instead of O(n).
use std::{ops::Index, vec};

use fxhash::FxBuildHasher;

use crate::Value;

type Big = im_rc::HashMap<String, Value, FxBuildHasher>;

const SMALL_MAX: usize = 8;

thread_local! {
  static POOL: im_rc::hashmap::HashMapPool<String, Value> =
    im_rc::hashmap::HashMapPool::new(4096);
}

#[derive(Clone, Debug)]
pub enum ValueMap {
  Small(Vec<(String, Value)>),
  Big(Big),
}

impl Default for ValueMap {
  fn default() -> Self {
    ValueMap::Small(Vec::new())
  }
}

impl ValueMap {
  pub fn get(&self, key: &str) -> Option<&Value> {
    match self {
      ValueMap::Small(v) => v.iter().find(|(k, _)| k == key).map(|(_, val)| val),
      ValueMap::Big(m) => m.get(key),
    }
  }

  pub fn contains_key(&self, key: &str) -> bool {
    self.get(key).is_some()
  }

  pub fn len(&self) -> usize {
    match self {
      ValueMap::Small(v) => v.len(),
      ValueMap::Big(m) => m.len(),
    }
  }

  pub fn is_empty(&self) -> bool {
    self.len() == 0
  }

  pub fn insert(&mut self, key: String, val: Value) -> Option<Value> {
    match self {
      ValueMap::Small(v) => {
        if let Some(slot) = v.iter_mut().find(|(k, _)| *k == key) {
          return Some(std::mem::replace(&mut slot.1, val));
        }
        if v.len() < SMALL_MAX {
          v.push((key, val));
          return None;
        }
        let mut big = POOL.with(|pool| Big::with_pool_hasher(pool, FxBuildHasher::default()));
        big.extend(v.drain(..));
        big.insert(key, val);
        *self = ValueMap::Big(big);
        None
      }
      ValueMap::Big(m) => m.insert(key, val),
    }
  }

  pub fn remove(&mut self, key: &str) -> Option<Value> {
    match self {
      ValueMap::Small(v) => v
        .iter()
        .position(|(k, _)| k == key)
        .map(|i| v.swap_remove(i).1),
      ValueMap::Big(m) => m.remove(key),
    }
  }

  pub fn iter(&self) -> Iter<'_> {
    match self {
      ValueMap::Small(v) => Iter::Small(v.iter()),
      ValueMap::Big(m) => Iter::Big(m.iter()),
    }
  }

  pub fn keys(&self) -> impl Iterator<Item = &String> {
    self.iter().map(|(k, _)| k)
  }

  pub fn values(&self) -> impl Iterator<Item = &Value> {
    self.iter().map(|(_, v)| v)
  }
}

impl Index<&str> for ValueMap {
  type Output = Value;

  fn index(&self, key: &str) -> &Value {
    self.get(key).expect("no entry found for key")
  }
}

pub enum Iter<'a> {
  Small(std::slice::Iter<'a, (String, Value)>),
  Big(im_rc::hashmap::Iter<'a, String, Value>),
}

impl<'a> Iterator for Iter<'a> {
  type Item = (&'a String, &'a Value);

  fn next(&mut self) -> Option<Self::Item> {
    match self {
      Iter::Small(it) => it.next().map(|(k, v)| (k, v)),
      Iter::Big(it) => it.next(),
    }
  }

  fn size_hint(&self) -> (usize, Option<usize>) {
    match self {
      Iter::Small(it) => it.size_hint(),
      Iter::Big(it) => it.size_hint(),
    }
  }
}

pub enum IntoIter {
  Small(vec::IntoIter<(String, Value)>),
  Big(im_rc::hashmap::ConsumingIter<(String, Value)>),
}

impl Iterator for IntoIter {
  type Item = (String, Value);

  fn next(&mut self) -> Option<Self::Item> {
    match self {
      IntoIter::Small(it) => it.next(),
      IntoIter::Big(it) => it.next(),
    }
  }
}

impl IntoIterator for ValueMap {
  type Item = (String, Value);
  type IntoIter = IntoIter;

  fn into_iter(self) -> IntoIter {
    match self {
      ValueMap::Small(v) => IntoIter::Small(v.into_iter()),
      ValueMap::Big(m) => IntoIter::Big(m.into_iter()),
    }
  }
}

impl<'a> IntoIterator for &'a ValueMap {
  type Item = (&'a String, &'a Value);
  type IntoIter = Iter<'a>;

  fn into_iter(self) -> Iter<'a> {
    self.iter()
  }
}

impl Extend<(String, Value)> for ValueMap {
  fn extend<I: IntoIterator<Item = (String, Value)>>(&mut self, iter: I) {
    for (k, v) in iter {
      self.insert(k, v);
    }
  }
}

impl FromIterator<(String, Value)> for ValueMap {
  fn from_iter<I: IntoIterator<Item = (String, Value)>>(iter: I) -> Self {
    let mut map = ValueMap::default();
    map.extend(iter);
    map
  }
}
