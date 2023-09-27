use algorithm::NoiseSource;

pub mod algorithm;
pub mod hill_noise;
pub mod interface;

unsafe fn vec_uninit<T>(len: usize) -> Vec<T> {
  let mut v = Vec::with_capacity(len);
  v.set_len(len);
  v
}

pub fn gen_heightmap(
  mut noise_source: NoiseSource,
  resolution: (usize, usize),
  (world_space_mins, world_space_maxs): ((f32, f32), (f32, f32)),
) -> Vec<f32> {
  let mut heightmap = unsafe { vec_uninit(resolution.1 * resolution.0) };

  let step_size_x = (world_space_maxs.0 - world_space_mins.0) / (resolution.0 - 1) as f32;
  let step_size_y = (world_space_maxs.1 - world_space_mins.1) / (resolution.1 - 1) as f32;

  let coords = (0..resolution.1).flat_map(|y| {
    (0..resolution.0).map(move |x| {
      (
        world_space_mins.0 + x as f32 * step_size_x,
        world_space_mins.1 + y as f32 * step_size_y,
      )
    })
  });
  noise_source.gen_batch(coords, &mut heightmap);

  heightmap
}
