//! Texture memory manager: every texture plane and texture-sized cache buffer is charged to a
//! per-thread ledger, and large allocations go through [`try_vec`], which on failure evicts
//! registered caches (oldest-value first) and retries before reporting what is live and where
//! it came from. The real limit is the allocator's (wasm's 4 GB linear memory, shared with
//! meshes and everything else), so there is no fixed texture budget; `set_limit` adds a soft
//! cap on tracked bytes.

use std::{
  cell::RefCell,
  fmt::Write,
  ops::{Deref, DerefMut},
  rc::{Rc, Weak},
};

use fxhash::FxHashMap;

use crate::ErrorStack;

/// A cache the manager may shrink under memory pressure.
pub trait Evict {
  /// Drops the least valuable entry; false once nothing is left (or the cache is in use).
  fn evict_one(&self) -> bool;
}

struct Entry {
  bytes: usize,
  kind: &'static str,
  origin: Origin,
}

/// Top-level statement that was executing when a buffer was charged.
#[derive(Clone, PartialEq, Eq, Hash, Default)]
struct Origin {
  module: Option<Rc<str>>,
  line: u32,
  run: u32,
}

#[derive(Default)]
struct Ledger {
  entries: FxHashMap<u32, Entry>,
  next_id: u32,
  live: usize,
  limit: Option<usize>,
  /// Sorted by priority; lower evicts first.
  caches: Vec<(u8, Weak<dyn Evict>)>,
  origin: Origin,
  evicted: usize,
}

thread_local! {
  static LEDGER: RefCell<Ledger> = RefCell::default();
}

/// RAII ledger entry: the charge lasts as long as the buffer that holds this.
pub struct Charge(u32);

impl Charge {
  pub fn new(kind: &'static str, bytes: usize) -> Self {
    LEDGER.with_borrow_mut(|l| {
      let id = l.next_id;
      l.next_id = l.next_id.wrapping_add(1);
      l.live += bytes;
      let origin = l.origin.clone();
      l.entries.insert(
        id,
        Entry {
          bytes,
          kind,
          origin,
        },
      );
      Charge(id)
    })
  }
}

impl Drop for Charge {
  fn drop(&mut self) {
    // `try_with`: thread teardown may drop buffers after the ledger itself.
    let _ = LEDGER.try_with(|l| {
      let mut l = l.borrow_mut();
      if let Some(e) = l.entries.remove(&self.0) {
        l.live -= e.bytes;
      }
    });
  }
}

/// Marks the start of a run, so buffers surviving from earlier ones are reported as such.
pub fn begin_run() {
  LEDGER.with_borrow_mut(|l| {
    l.origin = Origin {
      module: None,
      line: 0,
      run: l.origin.run.wrapping_add(1),
    };
    l.evicted = 0;
  });
}

pub fn set_origin(module: Option<&str>, line: u32) {
  LEDGER.with_borrow_mut(|l| {
    if l.origin.module.as_deref() != module {
      l.origin.module = module.map(Rc::from);
    }
    l.origin.line = line;
  });
}

/// Soft cap on tracked bytes; `None` leaves only the allocator's own limit.
pub fn set_limit(limit: Option<usize>) {
  LEDGER.with_borrow_mut(|l| l.limit = limit);
}

pub fn live_bytes() -> usize {
  LEDGER.with_borrow(|l| l.live)
}

pub fn register_cache(priority: u8, cache: Weak<dyn Evict>) {
  LEDGER.with_borrow_mut(|l| {
    l.caches.retain(|(_, c)| c.strong_count() > 0);
    let at = l.caches.partition_point(|(p, _)| *p <= priority);
    l.caches.insert(at, (priority, cache));
  });
}

fn evict_one() -> bool {
  // Cloned out: evicting drops buffers, whose charges re-enter the ledger.
  let caches: Vec<Rc<dyn Evict>> =
    LEDGER.with_borrow(|l| l.caches.iter().filter_map(|(_, c)| c.upgrade()).collect());
  let before = live_bytes();
  let hit = caches.iter().any(|c| c.evict_one());
  LEDGER.with_borrow_mut(|l| l.evicted += before.saturating_sub(l.live));
  hit
}

/// An empty vector with room for `len` elements, or an error describing tracked memory once
/// evicting every cache still leaves no room.
pub fn try_reserve<T>(len: usize) -> Result<Vec<T>, ErrorStack> {
  let bytes = len * size_of::<T>();
  loop {
    let fits = LEDGER.with_borrow(|l| l.limit.is_none_or(|lim| l.live + bytes <= lim));
    if fits {
      let mut v = Vec::new();
      if v.try_reserve_exact(len).is_ok() {
        return Ok(v);
      }
    }
    if !evict_one() {
      return Err(ErrorStack::new(report(bytes)));
    }
  }
}

pub fn try_vec<T: Clone>(len: usize, fill: T) -> Result<Vec<T>, ErrorStack> {
  let mut v = try_reserve(len)?;
  v.resize(len, fill);
  Ok(v)
}

/// `channels` zeroed planes of `len` texels, not yet charged: `TexStorage` adopts them.
pub fn try_planes(channels: usize, len: usize) -> Result<Vec<Vec<f32>>, ErrorStack> {
  (0..channels).map(|_| try_vec(len, 0.)).collect()
}

fn mb(bytes: usize) -> String {
  let m = bytes as f64 / (1 << 20) as f64;
  if m >= 1024. {
    format!("{:.2} GB", m / 1024.)
  } else if m >= 10. {
    format!("{m:.0} MB")
  } else {
    format!("{m:.1} MB")
  }
}

/// What a failed allocation of `bytes` was up against; also the body of the allocator's own
/// out-of-memory message, for allocations that never pass through the manager.
pub fn report(bytes: usize) -> String {
  LEDGER.with_borrow(|l| {
    let mut out = format!("Out of memory: could not allocate {}", mb(bytes));
    let at = |o: &Origin| match (&o.module, o.line) {
      (_, 0) => String::new(),
      (Some(m), line) => format!(" at {m}:{line}"),
      (None, line) => format!(" at line {line}"),
    };
    let _ = write!(out, "{}.", at(&l.origin));
    #[cfg(target_arch = "wasm32")]
    {
      let heap = core::arch::wasm32::memory_size(0) << 16;
      let _ = write!(out, "\nWasm heap: {} of 4 GB.", mb(heap));
    }
    let _ = write!(out, "\nTexture memory in use: {}", mb(l.live));
    if let Some(lim) = l.limit {
      let _ = write!(out, " of a {} limit", mb(lim));
    }
    if l.evicted > 0 {
      let _ = write!(out, " (after evicting {} of caches)", mb(l.evicted));
    }
    let mut groups: FxHashMap<(&'static str, &Origin), (usize, usize)> = FxHashMap::default();
    for e in l.entries.values() {
      let g = groups.entry((e.kind, &e.origin)).or_default();
      g.0 += e.bytes;
      g.1 += 1;
    }
    let mut groups: Vec<_> = groups.into_iter().collect();
    groups.sort_unstable_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0 .1.line.cmp(&b.0 .1.line)));
    const SHOWN: usize = 10;
    for ((kind, origin), (bytes, n)) in groups.iter().take(SHOWN) {
      let stale = if origin.run != l.origin.run {
        " (earlier evaluation)"
      } else {
        ""
      };
      let _ = write!(
        out,
        "\n  {:>8}  {kind} ×{n}{}{stale}",
        mb(*bytes),
        at(origin)
      );
    }
    if groups.len() > SHOWN {
      let rest: usize = groups[SHOWN..].iter().map(|g| g.1 .0).sum();
      let _ = write!(
        out,
        "\n  {:>8}  in {} smaller groups",
        mb(rest),
        groups.len() - SHOWN
      );
    }
    out
  })
}

/// One channel of texture storage. All planes are dense `f32` today; this type is the seam
/// where other backing formats would be introduced, which is why its buffer is private and its
/// size is asked for in bytes.
pub struct Plane {
  buf: Vec<f32>,
  _charge: Charge,
}

impl Plane {
  /// Takes ownership of an already-allocated buffer.
  pub fn adopt(buf: Vec<f32>) -> Self {
    Plane {
      _charge: Charge::new("texture plane", buf.capacity() * size_of::<f32>()),
      buf,
    }
  }

  pub fn zeroed(len: usize) -> Result<Self, ErrorStack> {
    Self::filled(len, 0.)
  }

  pub fn filled(len: usize, v: f32) -> Result<Self, ErrorStack> {
    try_vec(len, v).map(Self::adopt)
  }

  /// Collects exactly `len` values without zero-filling first.
  pub fn collect(len: usize, values: impl Iterator<Item = f32>) -> Result<Self, ErrorStack> {
    let mut buf = try_reserve(len)?;
    buf.extend(values.take(len));
    Ok(Self::adopt(buf))
  }

  pub fn try_clone(&self) -> Result<Self, ErrorStack> {
    Self::collect(self.buf.len(), self.buf.iter().copied())
  }

  pub fn bytes(&self) -> usize {
    self.buf.capacity() * size_of::<f32>()
  }

  pub fn as_slice(&self) -> &[f32] {
    &self.buf
  }

  pub fn into_vec(mut self) -> Vec<f32> {
    std::mem::take(&mut self.buf)
  }
}

impl Clone for Plane {
  fn clone(&self) -> Self {
    Self::adopt(self.buf.clone())
  }
}

impl Deref for Plane {
  type Target = [f32];

  fn deref(&self) -> &[f32] {
    &self.buf
  }
}

impl DerefMut for Plane {
  fn deref_mut(&mut self) -> &mut [f32] {
    &mut self.buf
  }
}

impl From<Vec<f32>> for Plane {
  fn from(buf: Vec<f32>) -> Self {
    Self::adopt(buf)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  struct Stash(RefCell<Vec<Plane>>);
  impl Evict for Stash {
    fn evict_one(&self) -> bool {
      self.0.borrow_mut().pop().is_some()
    }
  }

  #[test]
  fn evicts_caches_in_priority_order_then_reports() {
    begin_run();
    set_origin(Some("tex"), 7);
    let base = live_bytes();
    let stash = |n: usize| {
      Rc::new(Stash(RefCell::new(
        (0..n).map(|_| Plane::zeroed(1024).unwrap()).collect(),
      )))
    };
    let (early, late) = (stash(2), stash(2));
    register_cache(5, Rc::downgrade(&late) as Weak<dyn Evict>);
    register_cache(1, Rc::downgrade(&early) as Weak<dyn Evict>);
    let held = Plane::zeroed(1024).unwrap();
    assert_eq!(live_bytes() - base, 5 * 4096);

    set_limit(Some(base + 5 * 4096));
    set_origin(Some("main"), 12);
    let a = Plane::zeroed(2048).unwrap();
    assert_eq!((early.0.borrow().len(), late.0.borrow().len()), (0, 2));
    let b = Plane::zeroed(2048).unwrap();
    assert_eq!(late.0.borrow().len(), 0);

    let err = format!("{}", Plane::zeroed(1024).err().unwrap());
    assert!(
      err.contains("could not allocate 0.0 MB at main:12"),
      "{err}"
    );
    assert!(
      err.contains("after evicting") && err.contains("texture plane ×2 at main:12"),
      "{err}"
    );
    assert!(err.contains("texture plane ×1 at tex:7"), "{err}");
    set_limit(None);
    drop((held, a, b));
    assert_eq!(live_bytes(), base);
  }
}
