//! Cache-line-aligning global allocator, for wasm only.
//!
//! wasm has no mmap tier: dlmalloc serves every allocation from the single linear memory with
//! an 8-byte header, so two large buffers' relative alignment is whatever the allocation order
//! happens to produce. When a SIMD loop streams between two buffers whose 16-byte phases
//! differ, their vector accesses split cache lines on interleaved iterations. Measured cost on
//! Zen 4: up to 2.3x on a synthetic kernel, 10-19% on unmodified `ndarray` / `image` /
//! `rustfft`. Native is unaffected because glibc mmaps anything over 128 KB, so large buffers
//! are always page-aligned relative to each other -- hence the `wasm32` gate.
//!
//! Cost, measured: allocator CPU below noise, 0% memory in realistic churn (worst case +1.35%
//! with many simultaneously-live allocations at exactly `BIG`), +0.57% module size.
//!
//! Investigation and reproduction: `docs/wasm-perf-investigation-log.md`, `bench/wasm-alignment/`.

use std::alloc::{GlobalAlloc, Layout, System};

const LINE: usize = 64;
/// Below this the padding costs more than the cacheline effect can repay.
const BIG: usize = 4096;

pub struct CacheAligned;

/// Must be applied identically in every method -- `dealloc` has to see the layout `alloc` saw.
#[inline(always)]
fn bump(l: Layout) -> Layout {
  if l.size() >= BIG && l.align() < LINE {
    unsafe { Layout::from_size_align_unchecked(l.size(), LINE) }
  } else {
    l
  }
}

unsafe impl GlobalAlloc for CacheAligned {
  #[inline]
  unsafe fn alloc(&self, l: Layout) -> *mut u8 {
    System.alloc(bump(l))
  }

  #[inline]
  unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
    System.alloc_zeroed(bump(l))
  }

  #[inline]
  unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
    System.dealloc(p, bump(l))
  }

  #[inline]
  unsafe fn realloc(&self, p: *mut u8, l: Layout, new_size: usize) -> *mut u8 {
    let old = bump(l);
    let new = bump(Layout::from_size_align_unchecked(new_size, l.align()));
    if old.align() == new.align() {
      return System.realloc(p, old, new_size);
    }
    // crossed the threshold, so the alignment class changed: allocate, copy, free
    let np = System.alloc(new);
    if !np.is_null() {
      std::ptr::copy_nonoverlapping(p, np, l.size().min(new_size));
      System.dealloc(p, old);
    }
    np
  }
}
