pub extern crate rand;
pub extern crate rand_pcg;

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
    let rng = addr_of_mut!(RNG);
    for _ in 0..8 {
      let _ = (*rng).gen::<f32>();
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

pub fn uninit<T>() -> T {
  unsafe { std::mem::MaybeUninit::uninit().assume_init() }
}

pub fn clamp(val: f32, min: f32, max: f32) -> f32 {
  if val < min {
    return min;
  }
  if val > max {
    return max;
  }
  val
}

pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
  let t = clamp((x - edge0) / (edge1 - edge0), 0.0, 1.0);
  return t * t * (3.0 - 2.0 * t);
}
