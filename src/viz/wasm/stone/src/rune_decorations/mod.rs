use common::{rand::Rng, rand_pcg::Pcg32};
use lyon::{
  geom::LineSegment,
  lyon_tessellation::{
    geometry_builder::Positions, BuffersBuilder, StrokeOptions, StrokeTessellator, VertexBuffers,
  },
  math::{point, vector, Point, Vector},
  path::{builder::NoAttributes, path::BuilderImpl, Path},
};

pub mod exports;

pub(crate) struct RuneGenParams {
  pub segment_length: f32,
}

struct RuneGenCtx {
  pub rng: Pcg32,
  pub segments: Vec<LineSegment<f32>>,
  pub builder: NoAttributes<BuilderImpl>,
}

/// Converts a normalized direction vector to an angle in radians.
///
/// The angle is in the range [-PI, PI].
fn dir_to_angle(dir: Vector) -> f32 {
  dir.y.atan2(dir.x)
}

/// Converts an angle in radians to a normalized direction vector.
///
/// The angle is in the range [-PI, PI].
fn angle_to_dir(angle: f32) -> (f32, f32) {
  (angle.cos(), angle.sin())
}

const TWO_PI: f32 = std::f32::consts::PI * 2.0;

fn wrap_angle(angle: f32) -> f32 {
  let mut wrapped = angle;
  while wrapped >= std::f32::consts::PI {
    wrapped -= TWO_PI;
  }
  while wrapped < -std::f32::consts::PI {
    wrapped += TWO_PI;
  }
  wrapped
}

fn add_angles(a: f32, b: f32) -> f32 {
  wrap_angle(a + b)
}

impl RuneGenCtx {
  /// `start_pos` is the endpoint of the last segment from which to branch
  ///
  /// `dir` is the angle in radians of the direction of the last segment.
  pub fn build_subpath<'a>(
    &mut self,
    params: &'a RuneGenParams,
    start_pos: Point,
    dir: f32,
  ) -> impl FnMut(&mut RuneGenCtx) -> Option<Point> + 'a {
    let mut pos = start_pos;
    let mut dir = dir;
    let turn_dir = if self.rng.gen::<f32>() < 0.5 {
      -1.0
    } else {
      1.0
    };
    // in radians
    let delta_angle = self.rng.gen_range(0.04..0.2);
    let mut delta_angle = delta_angle * turn_dir;
    let delta_delta_angle = self.rng.gen_range(-0.01..0.01);

    // TODO: look into other methods of altering turn angle
    let get_new_turn_angle = move |delta_angle: &mut f32, cur_angle: f32| -> f32 {
      let new_turn_angle = add_angles(cur_angle, *delta_angle);
      *delta_angle += delta_delta_angle;
      new_turn_angle
    };

    let mut count = 0;
    move |ctx: &mut RuneGenCtx| {
      count += 1;
      if count > 500 {
        return None;
      }

      let dir_vec = angle_to_dir(dir);
      let next_pos = point(
        pos.x + dir_vec.0 * params.segment_length,
        pos.y + dir_vec.1 * params.segment_length,
      );
      let candidate_segment = LineSegment {
        from: pos,
        to: next_pos,
      };

      // Check if we're intersecting any other segment
      //
      // Brute force for now
      for segment in &ctx.segments {
        if segment.intersects(&candidate_segment) {
          return None;
        }
      }

      pos = next_pos;
      dir = get_new_turn_angle(&mut delta_angle, dir);
      Some(pos)
    }
  }

  pub fn add_subpath(&mut self, mut next_point: impl FnMut(&mut RuneGenCtx) -> Option<Point>) {
    let Some(start) = next_point(self) else {
      return;
    };
    self.builder.begin(start);

    let mut last = start;
    while let Some(point) = next_point(self) {
      self.segments.push(LineSegment {
        from: last,
        to: point,
      });
      self.builder.line_to(point);
      last = point;
    }
  }

  pub fn build_path(mut self, params: &RuneGenParams) -> Path {
    let start_pos = point(0.0, 0.0);
    let start_dir =
      vector(self.rng.gen_range(-1.0..1.0), self.rng.gen_range(-1.0..1.0)).normalize();
    let start_dir = dir_to_angle(start_dir);

    for _ in 0..1 {
      let subpath = self.build_subpath(params, start_pos, start_dir);
      self.add_subpath(subpath);
    }

    self.builder.build()
  }
}

fn build_path(params: &RuneGenParams) -> Path {
  let ctx = RuneGenCtx {
    rng: common::build_rng((8195444438u64, 382173857842u64)),
    segments: Vec::new(),
    builder: Path::builder(),
  };
  ctx.build_path(params)
}

fn build_and_tessellate_path(params: &RuneGenParams) -> VertexBuffers<Point, u32> {
  let path = build_path(params);

  let stroke_opts = StrokeOptions::default()
    .with_miter_limit(1.)
    .with_line_width(1.);
  let mut tessellator = StrokeTessellator::new();
  let mut buffers: VertexBuffers<Point, u32> = VertexBuffers::new();
  let mut vertex_builder: BuffersBuilder<Point, u32, _> =
    BuffersBuilder::new(&mut buffers, Positions);

  tessellator
    .tessellate_path(&path, &stroke_opts, &mut vertex_builder)
    .unwrap();
  buffers
}
