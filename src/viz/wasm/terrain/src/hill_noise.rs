//! Adapted from: https://blog.bruce-hill.com/hill-noise

use std::f32::consts::PI;

use rand::{Rng, SeedableRng};
use rand_pcg::Pcg32;

const SQRT_5: f32 = 2.2360679775;
const GOLDEN_RATIO: f32 = (SQRT_5 + 1.) / 2.;

pub struct HillNoise2D<const N: usize> {
  /// The wavelengths of each octave of the noise
  wavelengths: [f32; N],
  offsets: [(f32, f32); N],
  /// The standard deviation of the noise
  sigma: f32,
}

impl<const N: usize> HillNoise2D<N> {
  pub fn new(seed: u64, wavelengths: [f32; N]) -> Self {
    let mut rng = Pcg32::from_seed(unsafe { std::mem::transmute([seed, seed]) });
    // Returns a random number in the range [0, 1)
    let mut random = || -> f32 { rng.gen() };

    let offsets = {
      let mut offsets = [(0., 0.); N];
      for i in 0..N {
        unsafe {
          *offsets.get_unchecked_mut(i) = (random() * 2. * PI, random() * 2. * PI);
        }
      }
      offsets
    };

    let sigma = wavelengths
      .iter()
      .map(|&size| (size / 2.).powi(2))
      .sum::<f32>()
      .sqrt();

    Self {
      wavelengths,
      offsets,
      sigma,
    }
  }

  #[inline(never)]
  pub fn gen(&self, x: f32, y: f32) -> f32 {
    let mut noise = 0.0f32;

    for (i, &size) in self.wavelengths.iter().enumerate() {
      // Rotate coordinates
      let rotation = (i as f32 * GOLDEN_RATIO % 1.) * 2. * PI;
      // I've verified that these trig operations get pre-computed by the optimizer,
      // and manually pre-computing them actually causes more asm to be generated.
      let u = x * rotation.cos() - y * rotation.sin();
      let v = -x * rotation.sin() - y * rotation.cos();

      let offsets = unsafe { *self.offsets.get_unchecked(i) };
      noise += size / 2. * (f32::sin(u / size + offsets.0) + f32::sin(v / size + offsets.1));
    }

    // Approximate normal CDF:
    noise /= 2. * self.sigma;
    0.5 * (-1.0f32).copysign(noise) * (1. - (-2. / PI * noise * noise).exp()).sqrt() + 0.5
  }

  #[inline]
  pub fn gen_batch(
    &self,
    vals: impl Iterator<Item = (f32, f32)>,
    out: &mut [f32],
    multiplier: f32,
  ) {
    for (i, (x, y)) in vals.enumerate() {
      unsafe {
        *out.get_unchecked_mut(i) = self.gen(x, y) * multiplier;
      }
    }
  }
}

pub enum GenHillNoise2D {
  Oct1(HillNoise2D<1>),
  Oct2(HillNoise2D<2>),
  Oct3(HillNoise2D<3>),
  Oct4(HillNoise2D<4>),
  Oct5(HillNoise2D<5>),
  Oct6(HillNoise2D<6>),
  Oct7(HillNoise2D<7>),
  Oct8(HillNoise2D<8>),
  Oct9(HillNoise2D<9>),
  Oct10(HillNoise2D<10>),
  Oct11(HillNoise2D<11>),
  Oct12(HillNoise2D<12>),
  Oct13(HillNoise2D<13>),
  Oct14(HillNoise2D<14>),
  Oct15(HillNoise2D<15>),
  Oct16(HillNoise2D<16>),
}

impl GenHillNoise2D {
  pub fn new(octaves: u8, wavelengths: &[f32], seed: u64) -> Self {
    if wavelengths.len() != octaves as usize {
      panic!(
        "Number of wavelengths ({}) must match number of octaves ({})",
        wavelengths.len(),
        octaves
      );
    }

    unsafe {
      match octaves {
        1 => GenHillNoise2D::Oct1(HillNoise2D::new(seed, [*wavelengths.get_unchecked(0)])),
        2 => GenHillNoise2D::Oct2(HillNoise2D::new(
          seed,
          [*wavelengths.get_unchecked(0), *wavelengths.get_unchecked(1)],
        )),
        3 => GenHillNoise2D::Oct3(HillNoise2D::new(
          seed,
          [
            *wavelengths.get_unchecked(0),
            *wavelengths.get_unchecked(1),
            *wavelengths.get_unchecked(2),
          ],
        )),
        4 => GenHillNoise2D::Oct4(HillNoise2D::new(
          seed,
          [
            *wavelengths.get_unchecked(0),
            *wavelengths.get_unchecked(1),
            *wavelengths.get_unchecked(2),
            *wavelengths.get_unchecked(3),
          ],
        )),
        5 => GenHillNoise2D::Oct5(HillNoise2D::new(
          seed,
          [
            *wavelengths.get_unchecked(0),
            *wavelengths.get_unchecked(1),
            *wavelengths.get_unchecked(2),
            *wavelengths.get_unchecked(3),
            *wavelengths.get_unchecked(4),
          ],
        )),
        6 => GenHillNoise2D::Oct6(HillNoise2D::new(
          seed,
          [
            *wavelengths.get_unchecked(0),
            *wavelengths.get_unchecked(1),
            *wavelengths.get_unchecked(2),
            *wavelengths.get_unchecked(3),
            *wavelengths.get_unchecked(4),
            *wavelengths.get_unchecked(5),
          ],
        )),
        7 => GenHillNoise2D::Oct7(HillNoise2D::new(
          seed,
          [
            *wavelengths.get_unchecked(0),
            *wavelengths.get_unchecked(1),
            *wavelengths.get_unchecked(2),
            *wavelengths.get_unchecked(3),
            *wavelengths.get_unchecked(4),
            *wavelengths.get_unchecked(5),
            *wavelengths.get_unchecked(6),
          ],
        )),
        8 => GenHillNoise2D::Oct8(HillNoise2D::new(
          seed,
          [
            *wavelengths.get_unchecked(0),
            *wavelengths.get_unchecked(1),
            *wavelengths.get_unchecked(2),
            *wavelengths.get_unchecked(3),
            *wavelengths.get_unchecked(4),
            *wavelengths.get_unchecked(5),
            *wavelengths.get_unchecked(6),
            *wavelengths.get_unchecked(7),
          ],
        )),
        9 => GenHillNoise2D::Oct9(HillNoise2D::new(
          seed,
          [
            *wavelengths.get_unchecked(0),
            *wavelengths.get_unchecked(1),
            *wavelengths.get_unchecked(2),
            *wavelengths.get_unchecked(3),
            *wavelengths.get_unchecked(4),
            *wavelengths.get_unchecked(5),
            *wavelengths.get_unchecked(6),
            *wavelengths.get_unchecked(7),
            *wavelengths.get_unchecked(8),
          ],
        )),
        10 => GenHillNoise2D::Oct10(HillNoise2D::new(
          seed,
          [
            *wavelengths.get_unchecked(0),
            *wavelengths.get_unchecked(1),
            *wavelengths.get_unchecked(2),
            *wavelengths.get_unchecked(3),
            *wavelengths.get_unchecked(4),
            *wavelengths.get_unchecked(5),
            *wavelengths.get_unchecked(6),
            *wavelengths.get_unchecked(7),
            *wavelengths.get_unchecked(8),
            *wavelengths.get_unchecked(9),
          ],
        )),
        11 => GenHillNoise2D::Oct11(HillNoise2D::new(
          seed,
          [
            *wavelengths.get_unchecked(0),
            *wavelengths.get_unchecked(1),
            *wavelengths.get_unchecked(2),
            *wavelengths.get_unchecked(3),
            *wavelengths.get_unchecked(4),
            *wavelengths.get_unchecked(5),
            *wavelengths.get_unchecked(6),
            *wavelengths.get_unchecked(7),
            *wavelengths.get_unchecked(8),
            *wavelengths.get_unchecked(9),
            *wavelengths.get_unchecked(10),
          ],
        )),
        12 => GenHillNoise2D::Oct12(HillNoise2D::new(
          seed,
          [
            *wavelengths.get_unchecked(0),
            *wavelengths.get_unchecked(1),
            *wavelengths.get_unchecked(2),
            *wavelengths.get_unchecked(3),
            *wavelengths.get_unchecked(4),
            *wavelengths.get_unchecked(5),
            *wavelengths.get_unchecked(6),
            *wavelengths.get_unchecked(7),
            *wavelengths.get_unchecked(8),
            *wavelengths.get_unchecked(9),
            *wavelengths.get_unchecked(10),
            *wavelengths.get_unchecked(11),
          ],
        )),
        13 => GenHillNoise2D::Oct13(HillNoise2D::new(
          seed,
          [
            *wavelengths.get_unchecked(0),
            *wavelengths.get_unchecked(1),
            *wavelengths.get_unchecked(2),
            *wavelengths.get_unchecked(3),
            *wavelengths.get_unchecked(4),
            *wavelengths.get_unchecked(5),
            *wavelengths.get_unchecked(6),
            *wavelengths.get_unchecked(7),
            *wavelengths.get_unchecked(8),
            *wavelengths.get_unchecked(9),
            *wavelengths.get_unchecked(10),
            *wavelengths.get_unchecked(11),
            *wavelengths.get_unchecked(12),
          ],
        )),
        14 => GenHillNoise2D::Oct14(HillNoise2D::new(
          seed,
          [
            *wavelengths.get_unchecked(0),
            *wavelengths.get_unchecked(1),
            *wavelengths.get_unchecked(2),
            *wavelengths.get_unchecked(3),
            *wavelengths.get_unchecked(4),
            *wavelengths.get_unchecked(5),
            *wavelengths.get_unchecked(6),
            *wavelengths.get_unchecked(7),
            *wavelengths.get_unchecked(8),
            *wavelengths.get_unchecked(9),
            *wavelengths.get_unchecked(10),
            *wavelengths.get_unchecked(11),
            *wavelengths.get_unchecked(12),
            *wavelengths.get_unchecked(13),
          ],
        )),
        15 => GenHillNoise2D::Oct15(HillNoise2D::new(
          seed,
          [
            *wavelengths.get_unchecked(0),
            *wavelengths.get_unchecked(1),
            *wavelengths.get_unchecked(2),
            *wavelengths.get_unchecked(3),
            *wavelengths.get_unchecked(4),
            *wavelengths.get_unchecked(5),
            *wavelengths.get_unchecked(6),
            *wavelengths.get_unchecked(7),
            *wavelengths.get_unchecked(8),
            *wavelengths.get_unchecked(9),
            *wavelengths.get_unchecked(10),
            *wavelengths.get_unchecked(11),
            *wavelengths.get_unchecked(12),
            *wavelengths.get_unchecked(13),
            *wavelengths.get_unchecked(14),
          ],
        )),
        16 => GenHillNoise2D::Oct16(HillNoise2D::new(
          seed,
          [
            *wavelengths.get_unchecked(0),
            *wavelengths.get_unchecked(1),
            *wavelengths.get_unchecked(2),
            *wavelengths.get_unchecked(3),
            *wavelengths.get_unchecked(4),
            *wavelengths.get_unchecked(5),
            *wavelengths.get_unchecked(6),
            *wavelengths.get_unchecked(7),
            *wavelengths.get_unchecked(8),
            *wavelengths.get_unchecked(9),
            *wavelengths.get_unchecked(10),
            *wavelengths.get_unchecked(11),
            *wavelengths.get_unchecked(12),
            *wavelengths.get_unchecked(13),
            *wavelengths.get_unchecked(14),
            *wavelengths.get_unchecked(15),
          ],
        )),
        _ => panic!("Unsupported number of octaves: {}", octaves),
      }
    }
  }

  #[inline]
  pub fn gen(&self, x: f32, y: f32) -> f32 {
    match self {
      Self::Oct1(hill) => hill.gen(x, y),
      Self::Oct2(hill) => hill.gen(x, y),
      Self::Oct3(hill) => hill.gen(x, y),
      Self::Oct4(hill) => hill.gen(x, y),
      Self::Oct5(hill) => hill.gen(x, y),
      Self::Oct6(hill) => hill.gen(x, y),
      Self::Oct7(hill) => hill.gen(x, y),
      Self::Oct8(hill) => hill.gen(x, y),
      Self::Oct9(hill) => hill.gen(x, y),
      Self::Oct10(hill) => hill.gen(x, y),
      Self::Oct11(hill) => hill.gen(x, y),
      Self::Oct12(hill) => hill.gen(x, y),
      Self::Oct13(hill) => hill.gen(x, y),
      Self::Oct14(hill) => hill.gen(x, y),
      Self::Oct15(hill) => hill.gen(x, y),
      Self::Oct16(hill) => hill.gen(x, y),
    }
  }

  #[inline]
  pub fn gen_batch(
    &self,
    coords: impl Iterator<Item = (f32, f32)>,
    out: &mut [f32],
    multiplier: f32,
  ) {
    match self {
      Self::Oct1(hill) => hill.gen_batch(coords, out, multiplier),
      Self::Oct2(hill) => hill.gen_batch(coords, out, multiplier),
      Self::Oct3(hill) => hill.gen_batch(coords, out, multiplier),
      Self::Oct4(hill) => hill.gen_batch(coords, out, multiplier),
      Self::Oct5(hill) => hill.gen_batch(coords, out, multiplier),
      Self::Oct6(hill) => hill.gen_batch(coords, out, multiplier),
      Self::Oct7(hill) => hill.gen_batch(coords, out, multiplier),
      Self::Oct8(hill) => hill.gen_batch(coords, out, multiplier),
      Self::Oct9(hill) => hill.gen_batch(coords, out, multiplier),
      Self::Oct10(hill) => hill.gen_batch(coords, out, multiplier),
      Self::Oct11(hill) => hill.gen_batch(coords, out, multiplier),
      Self::Oct12(hill) => hill.gen_batch(coords, out, multiplier),
      Self::Oct13(hill) => hill.gen_batch(coords, out, multiplier),
      Self::Oct14(hill) => hill.gen_batch(coords, out, multiplier),
      Self::Oct15(hill) => hill.gen_batch(coords, out, multiplier),
      Self::Oct16(hill) => hill.gen_batch(coords, out, multiplier),
    }
  }
}
