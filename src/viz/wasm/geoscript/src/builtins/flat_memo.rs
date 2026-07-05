//! Shared helpers for thread-local memoization of pure Clipper2 results keyed on exact
//! input bit patterns.  Callers own a `thread_local! { static CACHE: RefCell<FlatMemoCache<..>> }`
//! and route through these so the size budget / packing conventions stay consistent.

use fxhash::FxHashMap;

/// Per-cache byte budget.  Entry size is dominated by the key (input coord bits) + output
/// polylines, so this bounds actual memory rather than entry count; typical profile ops are
/// ~1-4KB so the budget holds thousands of entries.  Eviction is wholesale: results are pure
/// and cheap to re-derive, so when the budget fills the whole map is dropped and re-warms.
const MAX_BYTES: usize = 16 * 1024 * 1024;

pub(crate) struct FlatMemoCache<V> {
  map: FxHashMap<Vec<u32>, V>,
  bytes: usize,
}

impl<V> Default for FlatMemoCache<V> {
  fn default() -> Self {
    FlatMemoCache {
      map: FxHashMap::default(),
      bytes: 0,
    }
  }
}

impl<V: Clone> FlatMemoCache<V> {
  pub fn get(&self, key: &[u32]) -> Option<V> {
    self.map.get(key).cloned()
  }

  pub fn insert(&mut self, key: Vec<u32>, val: &V, val_bytes: usize) {
    let entry_bytes = key.len() * 4 + val_bytes;
    if entry_bytes > MAX_BYTES {
      return;
    }
    if self.bytes + entry_bytes > MAX_BYTES {
      self.map.clear();
      self.bytes = 0;
    }
    self.bytes += entry_bytes;
    self.map.insert(key, val.clone());
  }
}

pub(crate) fn polylines_bytes(paths: &[Vec<crate::Vec2>]) -> usize {
  paths.iter().map(|p| p.len() * 8).sum()
}

pub(crate) fn push_f64_bits(key: &mut Vec<u32>, val: f64) {
  let bits = val.to_bits();
  key.push(bits as u32);
  key.push((bits >> 32) as u32);
}

pub(crate) fn push_f32_bits(key: &mut Vec<u32>, vals: &[f32]) {
  key.extend(vals.iter().map(|v| v.to_bits()));
}
