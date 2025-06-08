use lazy_static::lazy_static;
use mesh::linked_mesh::Vec3;
use noise::PermutationTable;

lazy_static! {
  static ref PERLIN_PERM_TABLE: PermutationTable = noise::PermutationTable::new(2882119348u32);
}

fn seed_offset(seed: u32) -> Vec3 {
  let h1 = seed.wrapping_mul(0x9E3779B1);
  let h2 = seed.wrapping_mul(0x85EBCA77);
  let h3 = seed.wrapping_mul(0xC2B2AE3D);

  Vec3::new(
    ((h1 >> 0) & 0xFFFF) as f32 / 65536. * 256.,
    ((h2 >> 8) & 0xFFFF) as f32 / 65536. * 256.,
    ((h3 >> 16) & 0xFFFF) as f32 / 65536. * 256.,
  )
}

pub fn perlin_noise(seed: u32, pos: Vec3) -> f32 {
  let pos = pos + seed_offset(seed);

  noise::perlin3(&PERLIN_PERM_TABLE, &[pos.x, pos.y, pos.z])
}

pub fn fbm(
  seed: u32,
  octaves: usize,
  frequency: f32,
  persistence: f32,
  lacunarity: f32,
  pos: Vec3,
) -> f32 {
  let mut value = 0.;
  let mut freq = frequency;
  let mut amp = 1.;

  for _ in 0..octaves {
    value += perlin_noise(seed, pos * freq) * amp;
    freq *= lacunarity;
    amp *= persistence;
  }

  value
}
