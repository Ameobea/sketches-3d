use std::ops::Mul;

use mesh::linked_mesh::Vec3;

use crate::Vec2;

#[allow(deprecated)]
type PermTable = noise::PermutationTable;

const PERLIN_PERM_TABLE: PermTable = unsafe {
  std::mem::transmute::<[u8; _], _>([
    246, 24, 167, 112, 231, 134, 42, 88, 182, 251, 121, 236, 125, 149, 31, 68, 62, 210, 113, 12,
    85, 96, 27, 18, 25, 191, 26, 173, 89, 225, 249, 101, 81, 32, 218, 205, 206, 207, 174, 192, 80,
    232, 226, 6, 30, 127, 74, 201, 2, 245, 184, 241, 223, 19, 144, 248, 48, 255, 146, 165, 102,
    109, 238, 235, 75, 53, 237, 76, 47, 214, 180, 129, 148, 70, 39, 254, 16, 138, 197, 29, 41, 150,
    0, 122, 242, 73, 161, 213, 87, 35, 115, 215, 11, 166, 98, 216, 37, 65, 56, 142, 9, 229, 93,
    195, 103, 178, 140, 136, 172, 247, 22, 155, 34, 154, 243, 224, 105, 253, 78, 94, 162, 193, 160,
    187, 55, 3, 49, 233, 86, 114, 5, 227, 36, 183, 118, 159, 230, 200, 61, 46, 38, 143, 7, 217, 83,
    119, 84, 28, 23, 104, 79, 8, 99, 66, 69, 239, 64, 133, 59, 58, 153, 17, 124, 240, 170, 40, 108,
    107, 219, 147, 185, 188, 52, 33, 158, 196, 176, 163, 151, 111, 135, 92, 10, 177, 169, 21, 228,
    117, 4, 175, 209, 198, 20, 120, 1, 43, 220, 106, 54, 186, 244, 44, 63, 130, 131, 199, 67, 110,
    71, 123, 189, 234, 157, 222, 45, 194, 128, 95, 252, 212, 204, 60, 152, 82, 116, 202, 156, 14,
    50, 190, 145, 179, 203, 139, 164, 77, 91, 221, 208, 171, 181, 137, 72, 57, 211, 97, 13, 126,
    141, 132, 100, 51, 250, 168, 90, 15,
  ])
};

fn seed_offset_3d(seed: u32) -> Vec3 {
  let h1 = seed.wrapping_mul(0x9E3779B1);
  let h2 = seed.wrapping_mul(0x85EBCA77);
  let h3 = seed.wrapping_mul(0xC2B2AE3D);

  Vec3::new(
    ((h1 >> 0) & 0xFFFF) as f32 / 65536. * 256.,
    ((h2 >> 8) & 0xFFFF) as f32 / 65536. * 256.,
    ((h3 >> 16) & 0xFFFF) as f32 / 65536. * 256.,
  )
}

fn seed_offset_2d(seed: u32) -> Vec2 {
  let h1 = seed.wrapping_mul(0x9E3779B1);
  let h2 = seed.wrapping_mul(0x85EBCA77);

  Vec2::new(
    ((h1 >> 0) & 0xFFFF) as f32 / 65536. * 256.,
    ((h2 >> 8) & 0xFFFF) as f32 / 65536. * 256.,
  )
}

pub fn perlin_noise_3d(seed: u32, pos: Vec3) -> f32 {
  let pos = pos + seed_offset_3d(seed);

  #[allow(deprecated)]
  noise::perlin3(&PERLIN_PERM_TABLE, &[pos.x, pos.y, pos.z])
}

pub fn perlin_noise_2d(seed: u32, pos: Vec2) -> f32 {
  let pos = pos + seed_offset_2d(seed);

  #[allow(deprecated)]
  noise::perlin2(&PERLIN_PERM_TABLE, &[pos.x, pos.y])
}

fn fbm_generic<P: Copy + Mul<f32, Output = P>>(
  perlin_fn: impl Fn(u32, P) -> f32,
  seed: u32,
  octaves: usize,
  frequency: f32,
  persistence: f32,
  lacunarity: f32,
  pos: P,
) -> f32 {
  let mut value = 0.;
  let mut freq = frequency;
  let mut amp = 1.;

  for octave_ix in 0..octaves {
    value += perlin_fn(seed + octave_ix as u32, pos * freq) * amp;
    freq *= lacunarity;
    amp *= persistence;
  }

  value
}

pub fn fbm_3d(
  seed: u32,
  octaves: usize,
  frequency: f32,
  persistence: f32,
  lacunarity: f32,
  pos: Vec3,
) -> f32 {
  fbm_generic(
    perlin_noise_3d,
    seed,
    octaves,
    frequency,
    persistence,
    lacunarity,
    pos,
  )
}

pub fn fbm_2d(
  seed: u32,
  octaves: usize,
  frequency: f32,
  persistence: f32,
  lacunarity: f32,
  pos: Vec2,
) -> f32 {
  fbm_generic(
    perlin_noise_2d,
    seed,
    octaves,
    frequency,
    persistence,
    lacunarity,
    pos,
  )
}

pub fn fbm_1d(
  seed: u32,
  octaves: usize,
  frequency: f32,
  persistence: f32,
  lacunarity: f32,
  pos: f32,
) -> f32 {
  fbm_generic(
    |seed, pos| perlin_noise_2d(seed, Vec2::new(pos, 0.)),
    seed,
    octaves,
    frequency,
    persistence,
    lacunarity,
    pos,
  )
}

const CURL_EPSILON: f32 = 0.001;

pub fn curl_noise_3d(
  seed: u32,
  octaves: usize,
  frequency: f32,
  persistence: f32,
  lacunarity: f32,
  pos: Vec3,
) -> Vec3 {
  let fbm = |seed, pos| fbm_3d(seed, octaves, frequency, persistence, lacunarity, pos);

  let eps_x = Vec3::new(CURL_EPSILON, 0., 0.);
  let eps_y = Vec3::new(0., CURL_EPSILON, 0.);
  let eps_z = Vec3::new(0., 0., CURL_EPSILON);

  let f_dy = (fbm(seed, pos + eps_y) - fbm(seed, pos - eps_y)) / (2. * CURL_EPSILON);
  let f_dz = (fbm(seed, pos + eps_z) - fbm(seed, pos - eps_z)) / (2. * CURL_EPSILON);

  let g_dx = (fbm(seed + 1, pos + eps_x) - fbm(seed + 1, pos - eps_x)) / (2. * CURL_EPSILON);
  let g_dz = (fbm(seed + 1, pos + eps_z) - fbm(seed + 1, pos - eps_z)) / (2. * CURL_EPSILON);

  let h_dx = (fbm(seed + 2, pos + eps_x) - fbm(seed + 2, pos - eps_x)) / (2. * CURL_EPSILON);
  let h_dy = (fbm(seed + 2, pos + eps_y) - fbm(seed + 2, pos - eps_y)) / (2. * CURL_EPSILON);

  Vec3::new(h_dy - g_dz, f_dz - h_dx, g_dx - f_dy)
}

pub fn curl_noise_2d(
  seed: u32,
  octaves: usize,
  frequency: f32,
  persistence: f32,
  lacunarity: f32,
  pos: Vec2,
) -> Vec2 {
  let fbm = |pos| fbm_2d(seed, octaves, frequency, persistence, lacunarity, pos);

  let eps_x = Vec2::new(CURL_EPSILON, 0.);
  let eps_y = Vec2::new(0., CURL_EPSILON);

  let deriv_x = (fbm(pos + eps_x) - fbm(pos - eps_x)) / (2. * CURL_EPSILON);
  let deriv_y = (fbm(pos + eps_y) - fbm(pos - eps_y)) / (2. * CURL_EPSILON);

  Vec2::new(deriv_y, -deriv_x)
}

pub fn ridged_3d(
  seed: u32,
  octaves: usize,
  frequency: f32,
  persistence: f32,
  lacunarity: f32,
  gain: f32,
  pos: Vec3,
) -> f32 {
  let mut value = 0.;
  let mut freq = frequency;
  let mut amp = 1.;
  let mut weight = 1.;

  for _ in 0..octaves {
    let mut signal = perlin_noise_3d(seed, pos * freq);
    signal = 1. - signal.abs();
    signal *= signal;
    signal *= weight;

    weight = (signal * gain).clamp(0., 1.);

    value += signal * amp;
    freq *= lacunarity;
    amp *= persistence;
  }

  value
}

pub fn ridged_2d(
  seed: u32,
  octaves: usize,
  frequency: f32,
  persistence: f32,
  lacunarity: f32,
  gain: f32,
  pos: Vec2,
) -> f32 {
  let mut value = 0.;
  let mut freq = frequency;
  let mut amp = 1.;
  let mut weight = 1.;

  for _ in 0..octaves {
    let mut signal = perlin_noise_2d(seed, pos * freq);
    signal = 1. - signal.abs();
    signal *= signal;
    signal *= weight;

    weight = (signal * gain).clamp(0., 1.);

    value += signal * amp;
    freq *= lacunarity;
    amp *= persistence;
  }

  value
}
