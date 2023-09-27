use nalgebra::{Rotation3, Transform3, Translation3};
use rand::{Rng, SeedableRng};
use rand_pcg::Pcg32;
use wasm_bindgen::prelude::*;

const START_POS: [f32; 3] = [167.53, 2.22, -36.28];
const END_POS: [f32; 3] = [275.98, -0.31, 86.86];

const PILLAR_ROWS_X: usize = 16;
const PILLAR_ROWS_Z: usize = 28;
const PILLAR_TYPE_COUNT: usize = 6;

#[derive(Default)]
pub struct PillarTypeState {
  pub transformations: Vec<nalgebra::Transform3<f32>>,
}

pub struct PillarCtx {
  pub rng: Pcg32,
  pub states: [PillarTypeState; PILLAR_TYPE_COUNT],
}

impl Default for PillarCtx {
  fn default() -> Self {
    #[allow(invalid_value)]
    let mut states: [PillarTypeState; PILLAR_TYPE_COUNT] =
      unsafe { std::mem::MaybeUninit::uninit().assume_init() };
    for i in 0..PILLAR_TYPE_COUNT {
      unsafe { std::ptr::write(states.get_unchecked_mut(i), PillarTypeState::default()) }
    }

    let mut rng =
      rand_pcg::Pcg32::from_seed(unsafe { std::mem::transmute([8938u64, 7827385782u64]) });

    let mut last_pillar_type: usize = 0;
    for x_row_ix in 0..PILLAR_ROWS_X {
      let x =
        START_POS[0] + (END_POS[0] - START_POS[0]) * (x_row_ix as f32 / (PILLAR_ROWS_X - 1) as f32);
      let base_y =
        START_POS[1] + (END_POS[1] - START_POS[1]) * (x_row_ix as f32 / (PILLAR_ROWS_Z - 1) as f32);

      for z_row_ix in 0..PILLAR_ROWS_Z {
        let z = START_POS[2]
          + (END_POS[2] - START_POS[2]) * (z_row_ix as f32 / (PILLAR_ROWS_Z - 1) as f32);
        let y = base_y + rng.gen_range(-8.0..8.);

        let mut pillar_type: usize = 0;
        for _ in 0..3 {
          pillar_type = rng.gen_range(0..PILLAR_TYPE_COUNT);
          if pillar_type == last_pillar_type && rng.gen_range(0..6usize) > 1 {
            pillar_type = rng.gen_range(0..PILLAR_TYPE_COUNT);
          } else {
            break;
          }
        }
        last_pillar_type = pillar_type;

        let mut transform: Transform3<f32> = nalgebra::Transform3::identity();
        transform *= Translation3::new(x, y, z);

        // Rotate randomly in 90 degree increments about the y axis
        transform *= Rotation3::from_euler_angles(
          0.,
          std::f32::consts::FRAC_PI_2 * rng.gen_range(0..4usize) as f32,
          0.,
        );

        states[pillar_type].transformations.push(transform);
      }
    }

    PillarCtx { rng, states }
  }
}

#[wasm_bindgen]
pub fn create_pillar_ctx() -> *const PillarCtx {
  Box::into_raw(Box::new(PillarCtx::default()))
}

#[wasm_bindgen]
pub fn compute_pillar_positions(_ctx: *mut PillarCtx) {
  // TODO
}

#[wasm_bindgen]
pub fn get_pillar_transformations(pillar_ctx: *const PillarCtx, pillar_type: usize) -> Vec<f32> {
  let pillar_ctx = unsafe { &*pillar_ctx };
  unsafe {
    std::slice::from_raw_parts(
      pillar_ctx.states[pillar_type].transformations.as_ptr() as *const f32,
      pillar_ctx.states[pillar_type].transformations.len() * 16,
    )
  }
  .to_owned()
}
