#![feature(iter_array_chunks)]

pub extern crate rand;
pub extern crate rand_pcg;

#[cfg(feature = "mesh")]
pub mod mesh;

use std::ptr::addr_of_mut;

use rand::prelude::*;
use rand_pcg::Pcg32;

static mut RNG: Pcg32 = unsafe { std::mem::transmute([(0u128)]) };

/// Must be called before using the RNG.
pub fn maybe_init_rng() {
  unsafe {
    if RNG != std::mem::transmute([(0u128)]) {
      return;
    }

    RNG = rand_pcg::Pcg32::from_seed(std::mem::transmute([89538u64, 382173857842u64]));

    // pump the rng a few times to avoid possible issues with seeding
    for _ in 0..8 {
      let _ = RNG.gen::<f32>();
    }
  }
}

#[inline(always)]
pub fn rng() -> &'static mut Pcg32 {
  unsafe { &mut *addr_of_mut!(RNG) }
}

pub fn build_rng(seed: (u64, u64)) -> Pcg32 {
  rand_pcg::Pcg32::from_seed(unsafe { std::mem::transmute(seed) })
}

/// Returns a random f32 in the range [0, 1).
#[inline(always)]
pub fn random() -> f32 {
  rng().gen::<f32>()
}
