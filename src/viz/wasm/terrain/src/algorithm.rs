use nanoserde::DeJson;
use opensimplex_noise_rs::OpenSimplexNoise;

use crate::hill_noise::GenHillNoise2D;

pub struct OctaveNoise<T> {
  pub noise: Vec<T>,
  pub coordinate_scales: Vec<f32>,
  pub weights: Vec<f32>,
}

impl<T> OctaveNoise<T> {
  pub fn gen(&mut self, x: f32, y: f32, sample_noise: impl Fn(&mut T, f32, f32) -> f32) -> f32 {
    let mut total = 0.0;
    for octave_ix in 0..self.noise.len() {
      let noise = &mut self.noise[octave_ix];
      let scale = self.coordinate_scales[octave_ix];
      let weight = self.weights[octave_ix];

      total += sample_noise(noise, x * scale, y * scale) * weight;
    }
    total
  }

  pub fn gen_batch(
    &mut self,
    positions: impl Iterator<Item = (f32, f32)>,
    out: &mut [f32],
    sample_noise: impl Fn(&mut T, f32, f32) -> f32,
  ) {
    for (i, (x, y)) in positions.enumerate() {
      let mut total = 0.0;
      for octave_ix in 0..self.noise.len() {
        let noise = &mut self.noise[octave_ix];
        let scale = self.coordinate_scales[octave_ix];
        let weight = self.weights[octave_ix];

        total += sample_noise(noise, x * scale, y * scale) * weight;
      }
      out[i] = total;
    }
  }
}

pub enum NoiseSourceInner {
  Hill(GenHillNoise2D),
  OpenSimplex {
    noise: OctaveNoise<OpenSimplexNoise>,
    magnitude: f32,
    offset_x: f32,
    offset_z: f32,
  },
}

impl NoiseSourceInner {
  pub fn gen(&mut self, x: f32, y: f32) -> f32 {
    match self {
      NoiseSourceInner::Hill(hill) => hill.gen(x, y),
      NoiseSourceInner::OpenSimplex {
        noise,
        magnitude,
        offset_x,
        offset_z,
      } => noise.gen(x, y, |noise, x, z| {
        let noise = noise.eval_2d((*offset_x + x) as f64, (*offset_z + z) as f64) as f32;
        noise * *magnitude
      }),
    }
  }

  pub fn gen_batch(
    &mut self,
    positions: impl Iterator<Item = (f32, f32)>,
    out: &mut [f32],
    multiplier: f32,
  ) {
    match self {
      NoiseSourceInner::Hill(hill) => hill.gen_batch(positions, out, multiplier),
      NoiseSourceInner::OpenSimplex {
        noise,
        magnitude,
        offset_x,
        offset_z,
      } => noise.gen_batch(
        positions.map(|(x, z)| (*offset_x + x, *offset_z + z)),
        out,
        |noise, x, y| noise.eval_2d(x as f64, y as f64) as f32 * *magnitude * multiplier,
      ),
    }
  }
}

pub struct NoiseSource {
  pub inner: NoiseSourceInner,
  pub magnitude: f32,
}

impl NoiseSource {
  pub fn gen(&mut self, x: f32, y: f32) -> f32 {
    self.inner.gen(x, y) * self.magnitude
  }

  pub fn gen_batch(&mut self, vals: impl Iterator<Item = (f32, f32)>, out: &mut [f32]) {
    self.inner.gen_batch(vals, out, self.magnitude);
  }
}

#[derive(DeJson)]
pub enum NoiseVariantParams {
  Hill {
    octaves: u8,
    wavelengths: Vec<f32>,
    seed: u64,
  },
  OpenSimplex {
    coordinate_scales: Vec<f32>,
    weights: Vec<f32>,
    seed: u64,
    magnitude: f32,
    offset_x: f32,
    offset_z: f32,
  },
}

#[derive(DeJson)]
pub struct NoiseParams {
  pub variant: NoiseVariantParams,
  pub magnitude: f32,
}

impl NoiseVariantParams {
  pub fn build(&self) -> NoiseSourceInner {
    match self {
      NoiseVariantParams::Hill {
        octaves,
        wavelengths,
        seed,
      } => NoiseSourceInner::Hill(GenHillNoise2D::new(*octaves, wavelengths, *seed)),
      NoiseVariantParams::OpenSimplex {
        coordinate_scales,
        weights,
        seed,
        magnitude,
        offset_x,
        offset_z,
      } => {
        let octaves = coordinate_scales.len();
        let mut noise = OctaveNoise {
          noise: Vec::with_capacity(octaves as usize),
          coordinate_scales: coordinate_scales.clone(),
          weights: weights.clone(),
        };
        for _ in 0..octaves {
          noise
            .noise
            .push(OpenSimplexNoise::new(Some(u64::cast_signed(*seed))));
        }
        NoiseSourceInner::OpenSimplex {
          noise,
          magnitude: *magnitude,
          offset_x: *offset_x,
          offset_z: *offset_z,
        }
      }
    }
  }
}

impl NoiseParams {
  pub fn build(&self) -> NoiseSource {
    NoiseSource {
      inner: self.variant.build(),
      magnitude: self.magnitude,
    }
  }
}
