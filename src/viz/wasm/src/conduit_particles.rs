use rand::{Rng, SeedableRng};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
  #[wasm_bindgen(js_namespace = console)]
  fn log(s: &str);
}

const MAX_PARTICLE_COUNT: usize = 80_000;
const PARTICLE_SPAWN_RATE_PER_SECOND: f32 = 300.;

const TARGET_DISTANCE_FROM_CONDUIT_WALL: f32 = 1.;

const DRAG_COEFFICIENT: f32 = 0.975;
const CONDUIT_ACCELERATION_PER_SECOND: f32 = 70.1;

type Vec3 = nalgebra::Vector3<f32>;

pub struct ConduitParticlesState {
  pub rng: rand_pcg::Pcg64,
  pub time_since_last_particle_spawn: f32,
  pub conduit_start_pos: Vec3,
  pub conduit_end_pos: Vec3,
  pub conduit_vector_normalized: Vec3,
  pub conduit_radius: f32,
  pub live_particle_count: usize,
  pub positions: Box<[Vec3; MAX_PARTICLE_COUNT]>,
  pub velocities: Box<[Vec3; MAX_PARTICLE_COUNT]>,
}

fn distance(a: &Vec3, b: &Vec3) -> f32 {
  (a.x - b.x).hypot(a.y - b.y)
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
  pub fn new(conduit_start_pos: Vec3, conduit_end_pos: Vec3, conduit_radius: f32) -> Self {
    let mut rng = rand_pcg::Pcg64::from_seed(unsafe {
      std::mem::transmute([8938u64, 7827385782u64, 101010101u64, 82392839u64])
    });

    // pump the rng a few times to avoid possible issues with seeding
    for _ in 0..8 {
      let _ = rng.gen::<f32>();
    }

    ConduitParticlesState {
      rng,
      time_since_last_particle_spawn: 0.,
      conduit_start_pos,
      conduit_end_pos,
      conduit_vector_normalized: (conduit_end_pos - conduit_start_pos).normalize(),
      conduit_radius,
      live_particle_count: 0,
      positions: box [Vec3::zeros(); MAX_PARTICLE_COUNT],
      velocities: box [Vec3::zeros(); MAX_PARTICLE_COUNT],
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
      *coord += self.rng.gen_range(-10.01..10.01) + cur_time_secs.sin() * self.conduit_radius;
    }
    self.velocities[particle_ix] = Vec3::new(new_velocity[0], new_velocity[1], new_velocity[2]);
  }

  fn spawn_particles(&mut self, cur_time_secs: f32, time_diff_secs: f32) {
    let spawn_rate_multiplier = (cur_time_secs * 2.).sin() * 0.5 + 0.5;

    self.time_since_last_particle_spawn += time_diff_secs;
    let particles_to_spawn = (self.time_since_last_particle_spawn
      * PARTICLE_SPAWN_RATE_PER_SECOND
      * spawn_rate_multiplier) as usize;
    self.time_since_last_particle_spawn -=
      particles_to_spawn as f32 / PARTICLE_SPAWN_RATE_PER_SECOND;
    for _ in 0..particles_to_spawn {
      self.spawn_particle(cur_time_secs);
    }
  }

  fn update_velocities(&mut self, cur_time_secs: f32, time_diff_secs: f32) {
    let extra_y_force = (cur_time_secs * 4.).sin();
    let extra_x_force = (cur_time_secs * 4.).cos();

    for particle_ix in 0..self.live_particle_count {
      let velocity = &mut self.velocities[particle_ix];
      *velocity *= DRAG_COEFFICIENT;

      let pos = self.positions[particle_ix];
      // Conduit center is represented as a line between the start and end points.
      let projected_pos = project_point_onto_line(
        &self.conduit_vector_normalized,
        &self.conduit_start_pos,
        &pos,
      );
      let distance_from_conduit_center = distance(&pos, &projected_pos);
      let distance_from_conduit_wall = distance_from_conduit_center - self.conduit_radius;
      // Negative if too close, positive if too far.
      let conduit_distance_error = distance_from_conduit_wall - TARGET_DISTANCE_FROM_CONDUIT_WALL;

      // We want to apply force to the particle to keep it near the desired
      // distance from the conduit wall. It will attract the particle
      // towards the conduit if it's too far away, and repel it if it's too
      // close.
      let conduit_normal_force_magnitude = (distance_from_conduit_wall).powi(2) * 1.4;
      let conduit_normal_force_direction = (pos - projected_pos).normalize();
      let conduit_normal_force =
        conduit_normal_force_direction * conduit_normal_force_magnitude * -conduit_distance_error;

      // Apply the force to the velocity.
      *velocity += conduit_normal_force * time_diff_secs;

      // Also apply force to move the particle along the conduit
      let conduit_travel_force = self.conduit_vector_normalized * CONDUIT_ACCELERATION_PER_SECOND;
      *velocity += conduit_travel_force * time_diff_secs;

      velocity.y += extra_y_force * time_diff_secs * 40.;
      velocity.y += -extra_y_force * time_diff_secs * 40.;
      velocity.z += extra_x_force * time_diff_secs * 40.;
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
      let should_despawn: bool = false; // TODO
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
}

#[wasm_bindgen]
pub fn create_conduit_particles_state(
  conduit_start_x: f32,
  conduit_start_y: f32,
  conduit_start_z: f32,
  conduit_end_x: f32,
  conduit_end_y: f32,
  conduit_end_z: f32,
  conduit_radius: f32,
) -> *mut ConduitParticlesState {
  std::panic::set_hook(Box::new(console_error_panic_hook::hook));

  Box::into_raw(box ConduitParticlesState::new(
    Vec3::new(conduit_start_x, conduit_start_y, conduit_start_z),
    Vec3::new(conduit_end_x, conduit_end_y, conduit_end_z),
    conduit_radius,
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
  let state: &'static mut _ = unsafe { &mut *state };
  state.tick(cur_time_secs, time_diff_secs);
  state.get_positions().to_owned()
}
