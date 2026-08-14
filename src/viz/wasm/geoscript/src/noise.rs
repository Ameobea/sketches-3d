use std::{f32::consts::FRAC_1_SQRT_2, ops::Mul};

use mesh::linked_mesh::Vec3;
use nalgebra::{Vector2, Vector3};

use crate::Vec2;

/// Worley distance metric (local port of the dropped `noise` crate's `RangeFunction`).
pub enum RangeFunction {
  Euclidean,
  EuclideanSquared,
  Manhattan,
  Chebyshev,
  Quadratic,
}

const PERLIN_PERM_TABLE: [u8; 256] = [
  246, 24, 167, 112, 231, 134, 42, 88, 182, 251, 121, 236, 125, 149, 31, 68, 62, 210, 113, 12, 85,
  96, 27, 18, 25, 191, 26, 173, 89, 225, 249, 101, 81, 32, 218, 205, 206, 207, 174, 192, 80, 232,
  226, 6, 30, 127, 74, 201, 2, 245, 184, 241, 223, 19, 144, 248, 48, 255, 146, 165, 102, 109, 238,
  235, 75, 53, 237, 76, 47, 214, 180, 129, 148, 70, 39, 254, 16, 138, 197, 29, 41, 150, 0, 122,
  242, 73, 161, 213, 87, 35, 115, 215, 11, 166, 98, 216, 37, 65, 56, 142, 9, 229, 93, 195, 103,
  178, 140, 136, 172, 247, 22, 155, 34, 154, 243, 224, 105, 253, 78, 94, 162, 193, 160, 187, 55,
  3, 49, 233, 86, 114, 5, 227, 36, 183, 118, 159, 230, 200, 61, 46, 38, 143, 7, 217, 83, 119, 84,
  28, 23, 104, 79, 8, 99, 66, 69, 239, 64, 133, 59, 58, 153, 17, 124, 240, 170, 40, 108, 107, 219,
  147, 185, 188, 52, 33, 158, 196, 176, 163, 151, 111, 135, 92, 10, 177, 169, 21, 228, 117, 4,
  175, 209, 198, 20, 120, 1, 43, 220, 106, 54, 186, 244, 44, 63, 130, 131, 199, 67, 110, 71, 123,
  189, 234, 157, 222, 45, 194, 128, 95, 252, 212, 204, 60, 152, 82, 116, 202, 156, 14, 50, 190,
  145, 179, 203, 139, 164, 77, 91, 221, 208, 171, 181, 137, 72, 57, 211, 97, 13, 126, 141, 132,
  100, 51, 250, 168, 90, 15,
];

// Byte-exact port of `noise` 0.4.1's surflet perlin (perlin2/perlin3 + PermutationTable
// hashing + gradient tables), kept f32-native — the reason the old crate was pinned.

#[inline(always)]
fn perm1(x: isize) -> usize {
  PERLIN_PERM_TABLE[(x & 0xff) as usize] as usize
}

#[inline(always)]
fn perm2(x: isize, y: isize) -> usize {
  PERLIN_PERM_TABLE[perm1(x) ^ ((y & 0xff) as usize)] as usize
}

#[inline(always)]
fn perm3(x: isize, y: isize, z: isize) -> usize {
  PERLIN_PERM_TABLE[perm2(x, y) ^ ((z & 0xff) as usize)] as usize
}

#[inline(always)]
fn grad2(index: usize) -> [f32; 2] {
  const NORM: f32 = 0.7071067811865475f64 as f32;
  match index % 8 {
    0 => [1., 0.],
    1 => [-1., 0.],
    2 => [0., 1.],
    3 => [0., -1.],
    4 => [NORM, NORM],
    5 => [-NORM, NORM],
    6 => [NORM, -NORM],
    7 => [-NORM, -NORM],
    _ => unreachable!(),
  }
}

#[inline(always)]
fn grad3(index: usize) -> [f32; 3] {
  const N: f32 = 0.7071067811865475f64 as f32;
  const N2: f32 = 0.5773502691896258f64 as f32;
  match index % 32 {
    // 12 edges repeated twice then 8 corners
    0 | 12 => [N, N, 0.],
    1 | 13 => [-N, N, 0.],
    2 | 14 => [N, -N, 0.],
    3 | 15 => [-N, -N, 0.],
    4 | 16 => [N, 0., N],
    5 | 17 => [-N, 0., N],
    6 | 18 => [N, 0., -N],
    7 | 19 => [-N, 0., -N],
    8 | 20 => [0., N, N],
    9 | 21 => [0., -N, N],
    10 | 22 => [0., N, -N],
    11 | 23 => [0., -N, -N],
    24 => [N2, N2, N2],
    25 => [-N2, N2, N2],
    26 => [N2, -N2, N2],
    27 => [-N2, -N2, N2],
    28 => [N2, N2, -N2],
    29 => [-N2, N2, -N2],
    30 => [N2, -N2, -N2],
    31 => [-N2, -N2, -N2],
    _ => unreachable!(),
  }
}

#[inline(always)]
fn surflet2(corner_hash: usize, dx: f32, dy: f32) -> f32 {
  let attn = 1. - (dx * dx + dy * dy);
  if attn > 0. {
    let g = grad2(corner_hash);
    (attn * attn * attn * attn) * (dx * g[0] + dy * g[1])
  } else {
    0.
  }
}

#[inline(always)]
fn surflet3(corner_hash: usize, dx: f32, dy: f32, dz: f32) -> f32 {
  let attn = 1. - (dx * dx + dy * dy + dz * dz);
  if attn > 0. {
    let g = grad3(corner_hash);
    (attn * attn * attn * attn) * (dx * g[0] + dy * g[1] + dz * g[2])
  } else {
    0.
  }
}

const PERLIN2_SCALE: f32 = 3.1604938271604937f64 as f32;
const PERLIN3_SCALE: f32 = 3.8898553255531074f64 as f32;

fn perlin2(x: f32, y: f32) -> f32 {
  let (fx, fy) = (x.floor(), y.floor());
  let (nx, ny) = (fx as isize, fy as isize);
  let (dx, dy) = (x - fx, y - fy);
  let (fdx, fdy) = (dx - 1., dy - 1.);

  let f00 = surflet2(perm2(nx, ny), dx, dy);
  let f10 = surflet2(perm2(nx + 1, ny), fdx, dy);
  let f01 = surflet2(perm2(nx, ny + 1), dx, fdy);
  let f11 = surflet2(perm2(nx + 1, ny + 1), fdx, fdy);

  (f00 + f10 + f01 + f11) * PERLIN2_SCALE
}

fn perlin3(x: f32, y: f32, z: f32) -> f32 {
  let (fx, fy, fz) = (x.floor(), y.floor(), z.floor());
  let (nx, ny, nz) = (fx as isize, fy as isize, fz as isize);
  let (dx, dy, dz) = (x - fx, y - fy, z - fz);
  let (fdx, fdy, fdz) = (dx - 1., dy - 1., dz - 1.);

  let f000 = surflet3(perm3(nx, ny, nz), dx, dy, dz);
  let f100 = surflet3(perm3(nx + 1, ny, nz), fdx, dy, dz);
  let f010 = surflet3(perm3(nx, ny + 1, nz), dx, fdy, dz);
  let f110 = surflet3(perm3(nx + 1, ny + 1, nz), fdx, fdy, dz);
  let f001 = surflet3(perm3(nx, ny, nz + 1), dx, dy, fdz);
  let f101 = surflet3(perm3(nx + 1, ny, nz + 1), fdx, dy, fdz);
  let f011 = surflet3(perm3(nx, ny + 1, nz + 1), dx, fdy, fdz);
  let f111 = surflet3(perm3(nx + 1, ny + 1, nz + 1), fdx, fdy, fdz);

  (f000 + f100 + f010 + f110 + f001 + f101 + f011 + f111) * PERLIN3_SCALE
}

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
  perlin3(pos.x, pos.y, pos.z)
}

pub fn perlin_noise_2d(seed: u32, pos: Vec2) -> f32 {
  let pos = pos + seed_offset_2d(seed);
  perlin2(pos.x, pos.y)
}

/// Exact-tiling perlin: corner indices wrap `mod period` before hashing, so the noise
/// repeats every `period` lattice cells. The fractional seed offset survives this — it
/// translates uniformly and cell indices advance by exactly `period` per tile.
fn periodic_perlin2(x: f32, y: f32, period: isize) -> f32 {
  let (fx, fy) = (x.floor(), y.floor());
  let (nx, ny) = (fx as isize, fy as isize);
  let (dx, dy) = (x - fx, y - fy);
  let (fdx, fdy) = (dx - 1., dy - 1.);
  let w = |c: isize| c.rem_euclid(period);

  let f00 = surflet2(perm2(w(nx), w(ny)), dx, dy);
  let f10 = surflet2(perm2(w(nx + 1), w(ny)), fdx, dy);
  let f01 = surflet2(perm2(w(nx), w(ny + 1)), dx, fdy);
  let f11 = surflet2(perm2(w(nx + 1), w(ny + 1)), fdx, fdy);

  (f00 + f10 + f01 + f11) * PERLIN2_SCALE
}

pub fn periodic_perlin_noise_2d(seed: u32, period: isize, pos: Vec2) -> f32 {
  let pos = pos + seed_offset_2d(seed);
  periodic_perlin2(pos.x, pos.y, period)
}

/// Seamlessly tiling fbm: `pos` spans `[0, period)` per tile. Each octave's frequency is
/// snapped so a whole number of lattice cells fits the period — that quantization is what
/// makes the tiling exact (a no-op for integer `period * frequency`).
pub fn fbm_2d_tileable(
  seed: u32,
  octaves: usize,
  frequency: f32,
  persistence: f32,
  lacunarity: f32,
  period: f32,
  pos: Vec2,
) -> f32 {
  let mut value = 0.;
  let mut freq = frequency;
  let mut amp = 1.;

  for octave_ix in 0..octaves {
    let cells = (period * freq).round().max(1.);
    value += periodic_perlin_noise_2d(seed + octave_ix as u32, cells as isize, pos * (cells / period))
      * amp;
    freq *= lacunarity;
    amp *= persistence;
  }

  value
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

// The following is adapted from: https://github.com/Razaekel/noise-rs/blob/develop/src/core/worley.rs

pub enum WorleyReturnType {
  Distance,
  Value,
}
impl WorleyReturnType {
  pub(crate) fn from_str(return_type: &str) -> Option<Self> {
    match return_type {
      "distance" => Some(WorleyReturnType::Distance),
      "value" => Some(WorleyReturnType::Value),
      _ => None,
    }
  }
}

fn hash_permtable(table: &[u8; 256], to_hash: &[isize]) -> usize {
  let index = to_hash
    .iter()
    .map(|&a| (a & 0xff) as usize)
    .reduce(|a, b| table[a] as usize ^ b)
    .unwrap();
  table[index] as usize
}

fn get_vec2(index: usize) -> Vector2<f32> {
  let length = ((index & 0xF8) >> 3) as f32 * 0.5 / 31.;
  let diag = length * FRAC_1_SQRT_2;

  Vector2::from(match index & 0x07 {
    0 => [diag, diag],
    1 => [diag, -diag],
    2 => [-diag, diag],
    3 => [-diag, -diag],
    4 => [length, 0.0],
    5 => [-length, 0.0],
    6 => [0.0, length],
    7 => [0.0, -length],
    _ => unreachable!(),
  })
}

fn get_vec3(index: usize) -> Vec3 {
  let length = ((index & 0xE0) >> 5) as f32 * 0.5 / 7.0;
  let diag = length * FRAC_1_SQRT_2;

  Vec3::from(match index % 18 {
    0 => [diag, diag, 0.0],
    1 => [diag, -diag, 0.0],
    2 => [-diag, diag, 0.0],
    3 => [-diag, -diag, 0.0],
    4 => [diag, 0.0, diag],
    5 => [diag, 0.0, -diag],
    6 => [-diag, 0.0, diag],
    7 => [-diag, 0.0, -diag],
    8 => [0.0, diag, diag],
    9 => [0.0, diag, -diag],
    10 => [0.0, -diag, diag],
    11 => [0.0, -diag, -diag],
    12 => [length, 0.0, 0.0],
    13 => [0.0, length, 0.0],
    14 => [0.0, 0.0, length],
    15 => [-length, 0.0, 0.0],
    16 => [0.0, -length, 0.0],
    17 => [0.0, 0.0, -length],
    _ => unreachable!("Attempt to access 3D gradient {} of 18", index % 18),
  })
}

fn worley_2d(
  distance_function: fn(&Vec2, &Vec2) -> f32,
  return_type: WorleyReturnType,
  point: Vec2,
) -> f32 {
  fn get_point(index: usize, whole: Vector2<isize>) -> Vector2<f32> {
    get_vec2(index) + Vector2::new(whole.x as f32, whole.y as f32)
  }

  let cell = Vector2::new(point.x.floor() as isize, point.y.floor() as isize);
  let floor = Vector2::new(cell.x as f32, cell.y as f32);
  let frac = point - floor;

  let half = frac.map(|x| x > 0.5);

  let near = half.map(|x| x as isize) + cell;
  let far = half.map(|x| !x as isize) + cell;

  let mut seed_cell = near;
  let seed_index = hash_permtable(&PERLIN_PERM_TABLE, near.as_slice());
  let seed_point = get_point(seed_index, near);
  let mut distance = distance_function(&point, &seed_point);

  let range = frac.map(|x| (0.5 - x).powf(2.0));

  macro_rules! test_point(
    [$x:expr, $y:expr] => {
      {
        let test_point = Vector2::from([$x, $y]);
        let index = hash_permtable(&PERLIN_PERM_TABLE, test_point.as_slice());
        let offset = get_point(index, test_point);
        let cur_distance = distance_function(&point, &offset);
        if cur_distance < distance {
          distance = cur_distance;
          seed_cell = test_point;
        }
      }
    }
  );

  if range.x < distance {
    test_point![far.x, near.y];
  }

  if range.y < distance {
    test_point![near.x, far.y];
  }

  if range.x < distance && range.y < distance {
    test_point![far.x, far.y];
  }

  let value = match return_type {
    WorleyReturnType::Distance => distance,
    WorleyReturnType::Value => {
      hash_permtable(&PERLIN_PERM_TABLE, seed_cell.as_slice()) as f32 / 255.0
    }
  };

  value * 2.0 - 1.0
}

fn worley_3d(
  distance_function: fn(&Vec3, &Vec3) -> f32,
  return_type: WorleyReturnType,
  point: Vec3,
) -> f32 {
  fn get_point(index: usize, whole: Vector3<isize>) -> Vector3<f32> {
    get_vec3(index) + Vector3::new(whole.x as f32, whole.y as f32, whole.z as f32)
  }

  let cell = Vector3::new(
    point.x.floor() as isize,
    point.y.floor() as isize,
    point.z.floor() as isize,
  );
  let floor = Vector3::new(cell.x as f32, cell.y as f32, cell.z as f32);
  let frac = point - floor;

  let half = frac.map(|x| x > 0.5);

  let near = half.map(|x| x as isize) + cell;
  let far = half.map(|x| !x as isize) + cell;

  let mut seed_cell = near;
  let seed_index = hash_permtable(&PERLIN_PERM_TABLE, near.as_slice());
  let seed_point = get_point(seed_index, near);
  let mut distance = distance_function(&point, &seed_point);

  let range = frac.map(|x| (0.5 - x).powf(2.0));

  macro_rules! test_point(
    [$x:expr, $y:expr, $z:expr] => {
      {
        let test_point = Vector3::from([$x, $y, $z]);
        let index = hash_permtable(&PERLIN_PERM_TABLE, test_point.as_slice());
        let offset = get_point(index, test_point);
        let cur_distance = distance_function(&point, &offset);
        if cur_distance < distance {
          distance = cur_distance;
          seed_cell = test_point;
        }
      }
    }
  );

  if range.x < distance {
    test_point![far.x, near.y, near.z];
  }
  if range.y < distance {
    test_point![near.x, far.y, near.z];
  }
  if range.z < distance {
    test_point![near.x, near.y, far.z];
  }

  if range.x < distance && range.y < distance {
    test_point![far.x, far.y, near.z];
  }
  if range.x < distance && range.z < distance {
    test_point![far.x, near.y, far.z];
  }
  if range.y < distance && range.z < distance {
    test_point![near.x, far.y, far.z];
  }

  if range.x < distance && range.y < distance && range.z < distance {
    test_point![far.x, far.y, far.z];
  }

  let value = match return_type {
    WorleyReturnType::Distance => distance,
    WorleyReturnType::Value => {
      hash_permtable(&PERLIN_PERM_TABLE, seed_cell.as_slice()) as f32 / 255.0
    }
  };

  value * 2.0 - 1.0
}

fn get_range_fn_impl_2d(range_fn: &RangeFunction) -> fn(&Vec2, &Vec2) -> f32 {
  match range_fn {
    RangeFunction::Euclidean => |a: &Vec2, b: &Vec2| (a - b).norm(),
    RangeFunction::Manhattan => |a: &Vec2, b: &Vec2| (a - b).abs().sum(),
    RangeFunction::Chebyshev => |a: &Vec2, b: &Vec2| (a - b).abs().max(),
    RangeFunction::EuclideanSquared => |a: &Vec2, b: &Vec2| (a - b).norm_squared(),
    RangeFunction::Quadratic => |a: &Vec2, b: &Vec2| (a - b).map(|x| x * x).sum(),
  }
}

fn get_range_fn_impl_3d(range_fn: &RangeFunction) -> fn(&Vec3, &Vec3) -> f32 {
  match range_fn {
    RangeFunction::Euclidean => |a: &Vec3, b: &Vec3| (a - b).norm(),
    RangeFunction::Manhattan => |a: &Vec3, b: &Vec3| (a - b).abs().sum(),
    RangeFunction::Chebyshev => |a: &Vec3, b: &Vec3| (a - b).abs().max(),
    RangeFunction::EuclideanSquared => |a: &Vec3, b: &Vec3| (a - b).norm_squared(),
    RangeFunction::Quadratic => |a: &Vec3, b: &Vec3| (a - b).map(|x| x * x).sum(),
  }
}

pub fn worley_noise_2d(
  seed: u32,
  pos: Vec2,
  range_fn: RangeFunction,
  return_type: WorleyReturnType,
) -> f32 {
  let pos = pos + seed_offset_2d(seed);
  worley_2d(get_range_fn_impl_2d(&range_fn), return_type, pos)
}

pub fn worley_noise_3d(
  seed: u32,
  pos: Vec3,
  range_fn: RangeFunction,
  return_type: WorleyReturnType,
) -> f32 {
  let pos = pos + seed_offset_3d(seed);
  worley_3d(get_range_fn_impl_3d(&range_fn), return_type, pos)
}

#[cfg(test)]
mod tests {
  use super::*;

  /// Golden values captured from `noise` 0.4.1 before the dep was dropped; the port was
  /// proven bit-identical over a dense grid at that point. These pin it forever.
  #[test]
  fn perlin_port_matches_noise_crate_goldens() {
    for (x, y, bits) in [
      (0.113f32, 300.7f32, 0xbe236298u32),
      (-7.31, 12.9, 0xbf00ad06),
      (251.77, -0.4, 0xbf45146b),
      (33.3, 77.7, 0x3f1b324a),
    ] {
      assert_eq!(perlin2(x, y).to_bits(), bits, "perlin2({x}, {y})");
    }
    for (x, y, z, bits) in [
      (0.05f32, -3.9f32, 251.3f32, 0x3f0c15b7u32),
      (-9.99, 8.1, 0.77, 0x3e259b35),
      (100.5, 200.25, -50.125, 0x3eb40057),
    ] {
      assert_eq!(perlin3(x, y, z).to_bits(), bits, "perlin3({x}, {y}, {z})");
    }
  }

  #[test]
  fn fbm_tileable_arg_dispatch() {
    let ctx = crate::parse_and_eval_program(
      r#"
a = fbm(pos=vec2(0., 0.3), tileable=true)
b = fbm(pos=vec2(1., 0.3), tileable=true)
c = fbm(seed=3, octaves=5, frequency=4., pos=vec2(0.2, 0.), tileable=2.5)
d = fbm(seed=3, octaves=5, frequency=4., pos=vec2(0.2, 2.5), tileable=2.5)
plain = fbm(pos=vec2(0.4, 0.7))
"#,
    )
    .unwrap();
    let getf = |name: &str| {
      ctx
        .globals
        .get(ctx.interned_symbols.intern(name))
        .unwrap()
        .as_float()
        .unwrap()
    };
    assert_eq!(getf("a").to_bits(), getf("b").to_bits());
    assert_eq!(getf("c").to_bits(), getf("d").to_bits());
    assert_eq!(getf("plain").to_bits(), fbm_2d(0, 4, 1., 0.5, 2., Vec2::new(0.4, 0.7)).to_bits());

    let err = crate::parse_and_eval_program("fbm(pos=vec2(0., 0.), tileable=-1.)").unwrap_err();
    assert!(err.to_string().contains("must be positive"));
  }

  #[test]
  fn tileable_fbm_is_exactly_periodic() {
    for period in [1., 2.5] {
      for i in 0..64 {
        let t = i as f32 / 64. * period;
        for (a, b) in [
          (Vec2::new(0., t), Vec2::new(period, t)),
          (Vec2::new(t, 0.), Vec2::new(t, period)),
        ] {
          let va = fbm_2d_tileable(7, 5, 3., 0.5, 2., period, a);
          let vb = fbm_2d_tileable(7, 5, 3., 0.5, 2., period, b);
          assert_eq!(va.to_bits(), vb.to_bits(), "period {period} at {a:?} vs {b:?}");
        }
      }
    }
  }
}
