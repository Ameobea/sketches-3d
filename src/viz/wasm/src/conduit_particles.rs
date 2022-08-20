use nanoserde::{DeJson, SerJson};
use noise::{Fbm, MultiFractal, NoiseModule};
use rand::{Rng, SeedableRng};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
  #[wasm_bindgen(js_namespace = console)]
  fn log(s: &str);
}

const MAX_PARTICLE_COUNT: usize = 80_000;

const TARGET_DISTANCE_FROM_CONDUIT_WALL: f32 = 1.;

type Vec3 = nalgebra::Vector3<f32>;

const RADAR_COLORS: &[[u8; 3]] = &[
  [0, 100, 100],
  // [0, 155, 155],
  [0, 255, 255],
  [0x00, 0x9e, 0xff],
  [0x00, 0x00, 0xff],
  [0x02, 0x83, 0xb1],
  [0x00, 0xff, 0x00],
  [0x01, 0xb1, 0x0c],
  [0xff, 0xd7, 0x00],
  [0xff, 0x99, 0x00],
  [0xff, 0x00, 0x00],
  [0xde, 0x00, 0x14],
  [0xbe, 0x00, 0x33],
  [0x79, 0x00, 0x6d],
  [0x79, 0x30, 0xa1],
  [0xc4, 0xa4, 0xd5],
];

// vec3 colors[5] = vec3[5](
//   vec3(0, 0, 4),
//   vec3(46, 8, 1),
//   vec3(105, 7, 2),
//   // vec3(31, 17, 87),
//   vec3(73, 2, 2),
//   vec3(43, 2, 2)
//   // vec3(65, 32, 129),
//   // vec3(18, 57, 73),
//   // vec3(33, 49, 136)
// );

const RED_COLORS: &[[u8; 3]] = &[
  [0, 0, 8],
  [46 * 2, 8 * 2, 1 * 2],
  [105 * 2, 7 * 2, 2 * 2],
  [73 * 2, 2 * 2, 2 * 2],
  [43 * 2, 2 * 2, 2 * 2],
];

const HEAT_COLORS: &[[u8; 3]] = &[
  [69, 38, 12],
  [135, 50, 7],
  [145, 123, 22],
  [145, 22, 22],
  [230, 21, 21],
  [204, 98, 98],
];

fn clamp(min: f32, max: f32, value: f32) -> f32 {
  if value < min {
    min
  } else if value > max {
    max
  } else {
    value
  }
}

fn mix_colors(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
  [
    (a[0] as f32 * (1. - t) + b[0] as f32 * t) as u8,
    (a[1] as f32 * (1. - t) + b[1] as f32 * t) as u8,
    (a[2] as f32 * (1. - t) + b[2] as f32 * t) as u8,
  ]
}

#[derive(Clone, SerJson, DeJson)]
pub struct ConduitParticlesConf {
  pub conduit_radius: f32,
  pub noise_frequency: f32,
  pub noise_amplitude: f32,
  pub noise_time_warp_speed: f32,
  pub drag_coefficient: f32,
  pub conduit_acceleration_per_second: f32,
  pub tidal_force_amplitude: f32,
  pub tidal_force_frequency: f32,
  pub particle_spawn_rate_per_second: f32,
  pub conduit_twist_frequency: f32,
  pub conduit_twist_amplitude: f32,
  pub conduit_attraction_magnitude: f32,
  pub noise_amplitude_modulation_frequency: f32,
  pub noise_amplitude_modulation_amplitude: f32,
}

impl Default for ConduitParticlesConf {
  fn default() -> Self {
    ConduitParticlesConf {
      conduit_radius: 6.96,
      noise_frequency: 2.,
      noise_amplitude: 1760.,
      noise_time_warp_speed: 0.1,
      drag_coefficient: 0.968,
      conduit_acceleration_per_second: 154.,
      tidal_force_amplitude: 80.,
      tidal_force_frequency: 1.83,
      particle_spawn_rate_per_second: 3000.,
      conduit_twist_frequency: 0.022,
      conduit_twist_amplitude: 12.,
      conduit_attraction_magnitude: 0.66,
      noise_amplitude_modulation_frequency: 1.5,
      noise_amplitude_modulation_amplitude: 400.,
    }
  }
}

pub struct ConduitParticlesState {
  pub conf: ConduitParticlesConf,
  pub conduit_ix: usize,
  pub rng: rand_pcg::Pcg64,
  pub time_since_last_particle_spawn: f32,
  pub conduit_start_pos: Vec3,
  pub conduit_end_pos: Vec3,
  pub conduit_vector_normalized: Vec3,
  pub live_particle_count: usize,
  pub positions: Box<[Vec3; MAX_PARTICLE_COUNT]>,
  pub velocities: Box<[Vec3; MAX_PARTICLE_COUNT]>,
  pub noise: noise::Perlin,
  pub perm_table: noise::PermutationTable,
}

fn distance(a: &Vec3, b: &Vec3) -> f32 {
  ((a.x - b.x) * (a.x - b.x) + (a.y - b.y) * (a.y - b.y) + (a.z - b.z) * (a.z - b.z)).sqrt()
}

/// Adapted from https://math.stackexchange.com/a/1905794/428311
///
/// Projects `point` onto the line defined by `line_start` and `line_end`.  The
/// distance between `point` and the line can be determined by taking the
/// distance between the returned projected point and `point`.
fn project_point_onto_line(conduit_vector_normalized: &Vec3, l1: &Vec3, point: &Vec3) -> Vec3 {
  let v = point - l1;
  let t = v.dot(&conduit_vector_normalized);
  let p = l1 + conduit_vector_normalized * t;
  p
}

impl ConduitParticlesState {
  pub fn new(conduit_start_pos: Vec3, conduit_end_pos: Vec3, conduit_ix: usize) -> Self {
    let mut rng = rand_pcg::Pcg64::from_seed(unsafe {
      std::mem::transmute([8938u64, 7827385782u64, 101010101u64, 82392839u64])
    });

    // pump the rng a few times to avoid possible issues with seeding
    for _ in 0..8 {
      let _ = rng.gen::<f32>();
    }

    let conf = ConduitParticlesConf::default();

    let mut noise = noise::Perlin::new();

    ConduitParticlesState {
      conf,
      conduit_ix,
      rng,
      time_since_last_particle_spawn: 0.,
      conduit_start_pos,
      conduit_end_pos,
      conduit_vector_normalized: (conduit_end_pos - conduit_start_pos).normalize(),
      live_particle_count: 0,
      positions: box [Vec3::zeros(); MAX_PARTICLE_COUNT],
      velocities: box [Vec3::zeros(); MAX_PARTICLE_COUNT],
      noise,
      perm_table: noise::PermutationTable::new(256),
    }
  }

  fn spawn_particle(&mut self, cur_time_secs: f32) {
    if self.live_particle_count >= MAX_PARTICLE_COUNT {
      return;
    }

    let mut initial_position = self.conduit_start_pos;
    for coord in &mut initial_position {
      *coord += self
        .rng
        //   .gen_range(-self.conduit_radius..self.conduit_radius);
        .gen_range(-3.01..3.01);
    }
    let particle_ix = self.live_particle_count;
    self.live_particle_count += 1;
    self.positions[particle_ix] = initial_position;

    let mut new_velocity = [0.0f32; 3];
    for coord in &mut new_velocity {
      *coord += self.rng.gen_range(-10.01..10.01) + cur_time_secs.sin() * self.conf.conduit_radius;
    }
    self.velocities[particle_ix] = Vec3::new(new_velocity[0], new_velocity[1], new_velocity[2]);
  }

  fn spawn_particles(&mut self, cur_time_secs: f32, time_diff_secs: f32) {
    let spawn_rate_multiplier = (cur_time_secs * 2.).sin() * 0.5 + 0.5;

    self.time_since_last_particle_spawn += time_diff_secs;
    let particles_to_spawn = (self.time_since_last_particle_spawn
      * self.conf.particle_spawn_rate_per_second
      * spawn_rate_multiplier) as usize;
    self.time_since_last_particle_spawn -=
      particles_to_spawn as f32 / self.conf.particle_spawn_rate_per_second;
    for _ in 0..particles_to_spawn {
      self.spawn_particle(cur_time_secs);
    }
  }

  #[inline(never)]
  fn get_noise(&self, pos: &Vec3, cur_time_secs: f32) -> f32 {
    // self.noise.get([
    //   pos.x * 0.012 * self.conf.noise_frequency + cur_time_secs *
    // self.conf.noise_time_warp_speed,   pos.y * 0.012 *
    // self.conf.noise_frequency + cur_time_secs * self.conf.noise_time_warp_speed,
    //   pos.z * 0.012 * self.conf.noise_frequency + cur_time_secs *
    // self.conf.noise_time_warp_speed, ]) as f32
    noise::open_simplex3(
      &self.perm_table,
      &[
        pos.x * 0.012 * self.conf.noise_frequency + cur_time_secs * self.conf.noise_time_warp_speed,
        pos.y * 0.012 * self.conf.noise_frequency + cur_time_secs * self.conf.noise_time_warp_speed,
        pos.z * 0.012 * self.conf.noise_frequency + cur_time_secs * self.conf.noise_time_warp_speed,
      ],
    )
  }

  #[inline(never)]
  fn update_velocities(&mut self, cur_time_secs: f32, time_diff_secs: f32) {
    let tidal_force_y = (cur_time_secs * self.conf.tidal_force_frequency).sin();
    let tidal_force_x = (cur_time_secs * self.conf.tidal_force_frequency).cos();

    for particle_ix in 0..self.live_particle_count {
      let pos = self.positions[particle_ix];
      let noise = self.get_noise(&pos, cur_time_secs);

      let velocity = &mut self.velocities[particle_ix];
      *velocity *= self.conf.drag_coefficient;

      // Conduit center is represented as a line between the start and end points.
      let mut projected_pos = project_point_onto_line(
        &self.conduit_vector_normalized,
        &self.conduit_start_pos,
        &pos,
      );

      // Offset projected pos based on distance from start point
      let distance_from_start = distance(&self.conduit_start_pos, &pos);
      projected_pos.y +=
        (distance_from_start * self.conf.conduit_twist_frequency + cur_time_secs * 0.4).sin()
          * self.conf.conduit_twist_amplitude;

      let distance_from_conduit_center = distance(&pos, &projected_pos);
      let distance_from_conduit_wall = distance_from_conduit_center - self.conf.conduit_radius;
      // Negative if too close, positive if too far.
      let conduit_distance_error = distance_from_conduit_wall - TARGET_DISTANCE_FROM_CONDUIT_WALL;

      // We want to apply force to the particle to keep it near the desired
      // distance from the conduit wall. It will attract the particle
      // towards the conduit if it's too far away, and repel it if it's too
      // close.
      let conduit_normal_force_magnitude = distance_from_conduit_wall
        * distance_from_conduit_wall
        * self.conf.conduit_attraction_magnitude;
      let conduit_normal_force_direction = (pos - projected_pos).normalize();
      let conduit_normal_force =
        conduit_normal_force_direction * conduit_normal_force_magnitude * -conduit_distance_error;

      // Apply the force to the velocity.
      *velocity += conduit_normal_force * time_diff_secs;

      // Also apply force to move the particle along the conduit
      let conduit_travel_force =
        self.conduit_vector_normalized * self.conf.conduit_acceleration_per_second;
      *velocity += conduit_travel_force * time_diff_secs;

      let mut noise_amplitude = self.conf.noise_amplitude;
      noise_amplitude += (cur_time_secs * self.conf.noise_amplitude_modulation_frequency).sin()
        * self.conf.noise_amplitude_modulation_amplitude;

      let noise = noise * time_diff_secs * noise_amplitude;
      velocity.y += noise;

      // velocity.y += extra_y_force * time_diff_secs * 40.;
      velocity.x += -tidal_force_y * time_diff_secs * self.conf.tidal_force_amplitude;
      velocity.z += tidal_force_x * time_diff_secs * self.conf.tidal_force_amplitude;
    }
  }

  fn update_positions(&mut self, time_diff_secs: f32) {
    for particle_ix in 0..self.live_particle_count {
      let particle_pos = &mut self.positions[particle_ix];
      let particle_vel = &self.velocities[particle_ix];
      *particle_pos += *particle_vel * time_diff_secs;
    }
  }

  fn despawn_particles(&mut self) {
    let mut particle_ix = 0;
    while particle_ix < self.live_particle_count {
      let pos = &self.positions[particle_ix];
      let should_despawn: bool = pos.x > 400.
        || pos.z > 400.
        || pos.x < -10_000.
        || pos.y < -10_000.
        || pos.z < -10_000.
        || pos.y > 10_000.;
      if !should_despawn {
        particle_ix += 1;
        continue;
      }

      self.positions[particle_ix] = self.positions[self.live_particle_count - 1];
      self.velocities[particle_ix] = self.velocities[self.live_particle_count - 1];
      self.live_particle_count -= 1;
    }
  }

  pub fn tick(&mut self, cur_time_secs: f32, time_diff_secs: f32) {
    self.spawn_particles(cur_time_secs, time_diff_secs);
    self.update_velocities(cur_time_secs, time_diff_secs);
    self.update_positions(time_diff_secs);
    self.despawn_particles();
  }

  pub fn get_positions(&self) -> &[f32] {
    unsafe {
      std::slice::from_raw_parts(
        self.positions.as_ptr() as *const f32,
        (self.live_particle_count + 1) * 3,
      )
    }
  }

  fn set_conf(&mut self, new_conf: ConduitParticlesConf) {
    self.conf = new_conf.clone();
  }

  fn get_particle_color(velocity: &Vec3) -> [u8; 3] {
    let velocity_range = [0., 650.];
    let velocity_magnitude = velocity.magnitude();
    // Clamp velocity magnitude to range.
    let velocity_magnitude = clamp(velocity_range[0], velocity_range[1], velocity_magnitude);

    let colors = RADAR_COLORS;
    // Map from `velocity_range` to [0, colors.len() - 1]
    let color_ix = (velocity_magnitude - velocity_range[0])
      / (velocity_range[1] - velocity_range[0])
      * (colors.len() as f32 - 1.);
    let color_ix_floor = color_ix.floor();
    let color_ix_fract = color_ix - color_ix_floor;
    let color_0_ix = color_ix_floor as usize;
    let color_1_ix = color_0_ix + 1;
    let color_0 = colors[color_0_ix];
    let color_1 = colors.get(color_1_ix).copied().unwrap_or([0, 0, 0]);
    let color = mix_colors(color_0, color_1, color_ix_fract);
    color
  }

  pub fn get_particle_colors(&self) -> Vec<u8> {
    let mut colors: Vec<u8> = Vec::with_capacity(self.live_particle_count * 4);
    for particle_ix in 0..self.live_particle_count {
      let velocity = &self.velocities[particle_ix];
      let particle_color = Self::get_particle_color(velocity);
      colors.extend_from_slice(&particle_color);
    }
    colors
  }
}

#[wasm_bindgen]
pub fn create_conduit_particles_state(
  conduit_start_x: f32,
  conduit_start_y: f32,
  conduit_start_z: f32,
  conduit_end_x: f32,
  conduit_end_y: f32,
  conduit_end_z: f32,
  conduit_ix: usize,
) -> *mut ConduitParticlesState {
  std::panic::set_hook(Box::new(console_error_panic_hook::hook));

  Box::into_raw(box ConduitParticlesState::new(
    Vec3::new(conduit_start_x, conduit_start_y, conduit_start_z),
    Vec3::new(conduit_end_x, conduit_end_y, conduit_end_z),
    conduit_ix,
  ))
}

#[wasm_bindgen]
pub fn free_conduit_particles_state(state: *mut ConduitParticlesState) {
  unsafe {
    drop(Box::from_raw(state));
  }
}

/// Ticks the simulation forward by one frame and returns the new positions of
/// all particles.
#[wasm_bindgen]
pub fn tick_conduit_particles(
  state: *mut ConduitParticlesState,
  cur_time_secs: f32,
  time_diff_secs: f32,
) -> Vec<f32> {
  let state = unsafe { &mut *state };
  state.tick(cur_time_secs, time_diff_secs);
  state.get_positions().to_owned()
}

#[wasm_bindgen]
pub fn set_conduit_conf(state: *mut ConduitParticlesState, conf_json: &str) {
  let state = unsafe { &mut *state };
  state.set_conf(DeJson::deserialize_json(conf_json).unwrap());
}

#[wasm_bindgen]
pub fn get_default_conduit_conf_json() -> String {
  ConduitParticlesConf::default().serialize_json()
}

#[wasm_bindgen]
pub fn get_current_conduit_rendered_particle_count(state: *mut ConduitParticlesState) -> usize {
  let state = unsafe { &mut *state };
  state.live_particle_count
}

#[wasm_bindgen]
pub fn get_conduit_particle_colors(state: *mut ConduitParticlesState) -> Vec<u8> {
  let state = unsafe { &mut *state };
  state.get_particle_colors().to_owned()
}
