use nanoserde::DeJson;

use crate::hill_noise::GenHillNoise2D;

pub enum NoiseSource {
  Hill(GenHillNoise2D),
}

impl NoiseSource {
  pub fn gen(&mut self, x: f32, y: f32) -> f32 {
    match self {
      NoiseSource::Hill(hill) => hill.gen(x, y),
    }
  }

  pub fn gen_batch(&mut self, vals: impl Iterator<Item = (f32, f32)>, out: &mut [f32]) {
    match self {
      NoiseSource::Hill(hill) => hill.gen_batch(vals, out),
    }
  }
}

#[derive(DeJson)]
pub enum NoiseParams {
  Hill {
    octaves: u8,
    wavelengths: Vec<f32>,
    seed: u64,
  },
}

impl NoiseParams {
  pub fn build(&self) -> NoiseSource {
    match self {
      NoiseParams::Hill {
        octaves,
        wavelengths,
        seed,
      } => NoiseSource::Hill(GenHillNoise2D::new(*octaves, wavelengths, *seed)),
    }
  }
}
