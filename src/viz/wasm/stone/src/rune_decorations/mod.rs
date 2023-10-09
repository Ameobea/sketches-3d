use std::ops::ControlFlow;

use common::{rand::Rng, rand_pcg::Pcg32};
use const_chunks::IteratorConstChunks;
use fnv::FnvHashMap;
use lyon::{
  geom::{
    euclid::{Point3D, UnknownUnit},
    LineSegment,
  },
  lyon_tessellation::{
    geometry_builder::Positions, BuffersBuilder, StrokeOptions, StrokeTessellator, VertexBuffers,
  },
  math::{point, vector, Point, Vector},
  path::{builder::NoAttributes, path::BuilderImpl, Path},
};

use crate::aabb_tree::{AABBTree, AABB};

pub mod exports;

pub(crate) struct RuneGenParams {
  pub segment_length: f32,
  pub subpath_count: usize,
  pub extrude_height: f32,
}

struct RuneSegment {
  pub seg: LineSegment<f32>,
  pub delta_angle: f32,
}

struct RuneGenCtx {
  pub rng: Pcg32,
  pub segments: Vec<RuneSegment>,
  pub aabb_tree: AABBTree<usize>,
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

fn seg_aabb(seg: &LineSegment<f32>) -> AABB {
  AABB {
    min: [seg.from.x.min(seg.to.x), seg.from.y.min(seg.to.y)],
    max: [seg.from.x.max(seg.to.x), seg.from.y.max(seg.to.y)],
  }
}

enum SegGenOutcome {
  Some(RuneSegment),
  End,
  Restart,
}

impl RuneGenCtx {
  /// `start_pos` is the endpoint of the last segment from which to branch
  ///
  /// `dir` is the angle in radians of the direction of the last segment.
  pub fn build_subpath<'a>(
    &mut self,
    params: &'a RuneGenParams,
    start_pos: Point,
    start_delta_angle: Option<f32>,
    dir: f32,
  ) -> impl FnMut(&[RuneSegment], &mut RuneGenCtx) -> SegGenOutcome + 'a {
    let mut pos = start_pos;
    let mut dir = dir;
    let mut delta_angle = match start_delta_angle {
      Some(start_delta_angle) => start_delta_angle,
      None => {
        let turn_dir = if self.rng.gen::<f32>() < 0.5 {
          -1.0
        } else {
          1.0
        };
        // in radians
        let delta_angle = self.rng.gen_range(0.04..0.9);
        delta_angle * turn_dir
      }
    };
    let delta_delta_angle = self.rng.gen_range(-0.09..0.09);

    // TODO: look into other methods of altering turn angle
    let get_new_turn_angle = move |rng: &mut Pcg32, delta_angle: &mut f32, cur_angle: f32| -> f32 {
      let new_turn_angle = add_angles(cur_angle, *delta_angle);
      *delta_angle += delta_delta_angle;
      if rng.gen::<f32>() < 0.02 {
        *delta_angle *= -1.0;
      }
      new_turn_angle
    };

    let mut count = 0;
    move |generated_so_far: &[RuneSegment], ctx: &mut RuneGenCtx| {
      count += 1;
      if count > 2500 {
        return SegGenOutcome::End;
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
      let aabb = seg_aabb(&candidate_segment);
      let mut intersects = generated_so_far
        .iter()
        .any(|seg| seg.seg.intersects(&candidate_segment));

      if !intersects {
        ctx.aabb_tree.query(&aabb, |_other_aabb, other_seg_ix| {
          let other_seg = &ctx.segments[*other_seg_ix];
          if other_seg.seg.intersects(&candidate_segment) {
            intersects = true;
            return ControlFlow::Break(());
          }
          ControlFlow::Continue(())
        });
      }
      if intersects {
        if count < 25 {
          return SegGenOutcome::Restart;
        }
        return SegGenOutcome::End;
      }

      pos = next_pos;
      dir = get_new_turn_angle(&mut ctx.rng, &mut delta_angle, dir);
      SegGenOutcome::Some(RuneSegment {
        seg: candidate_segment,
        delta_angle,
      })
    }
  }

  pub fn add_subpath(&mut self, params: &RuneGenParams) {
    let mut generated = Vec::new();
    let mut restart_count = 0;
    'outer: loop {
      let (start_pos, start_delta_angle, start_dir) = if self.segments.is_empty() {
        let start_pos = point(0.0, 0.0);
        let start_dir =
          vector(self.rng.gen_range(-1.0..1.0), self.rng.gen_range(-1.0..1.0)).normalize();
        let start_dir = dir_to_angle(start_dir);
        (start_pos, None, start_dir)
      } else {
        let starting_seg_ix = self.rng.gen_range(0..self.segments.len());
        let starting_seg = &self.segments[starting_seg_ix];
        let start_pos = starting_seg.seg.to;
        // if we were turning left, turn right the same amount, and vice versa
        let start_delta_angle = Some(-starting_seg.delta_angle);
        let start_dir = dir_to_angle(starting_seg.seg.to - starting_seg.seg.from);
        (start_pos, start_delta_angle, start_dir)
      };

      let mut next_seg = self.build_subpath(params, start_pos, start_delta_angle, start_dir);
      generated.clear();

      'inner: loop {
        match next_seg(&generated, self) {
          SegGenOutcome::Some(seg) => {
            generated.push(seg);
          }
          SegGenOutcome::End => {
            break 'outer;
          }
          SegGenOutcome::Restart => {
            restart_count += 1;
            if restart_count > 1000 {
              panic!("too many restarts");
            }
            break 'inner;
          }
        }
      }
    }

    let mut generated = generated.into_iter();
    let Some(first) = generated.next() else {
      return;
    };
    self.builder.begin(first.seg.from);
    self.builder.line_to(first.seg.to);
    let mut last = first.seg.to;

    for seg in generated {
      if seg.seg.from != last {
        self.builder.end(false);
        self.builder.begin(seg.seg.from);
      }
      self.builder.line_to(seg.seg.to);
      last = seg.seg.to;
      let seg_ix = self.segments.len();
      let aabb = seg_aabb(&seg.seg);
      self.segments.push(seg);
      self.aabb_tree.insert(aabb, seg_ix);
    }
    self.builder.end(false);
  }

  pub fn populate(&mut self, params: &RuneGenParams) {
    self.add_subpath(params);

    for i in 0..params.subpath_count {
      self.add_subpath(params);
      if (i + 1) == 100 {
        self.aabb_tree.balance();
      }
    }
  }

  pub fn build_path(mut self, params: &RuneGenParams) -> Path {
    self.populate(params);

    self.builder.build()
  }
}

fn build_path(params: &RuneGenParams) -> Path {
  let ctx = RuneGenCtx {
    rng: common::build_rng((8195444438u64, 382173857842u64)),
    segments: Vec::new(),
    aabb_tree: AABBTree::new(),
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

fn extrude_to_3d(
  buffers: VertexBuffers<Point, u32>,
  height: f32,
) -> VertexBuffers<Point3D<f32, UnknownUnit>, u32> {
  let mut vertices_3d = Vec::with_capacity(buffers.vertices.len() * 2);
  // TODO: pre-compute size
  let mut indices_3d = Vec::new();

  let vertex_count_2d = buffers.vertices.len() as u32;
  let mut add_face = |i: u32, j: u32, k: u32, flip: bool| {
    indices_3d.push(i);
    if flip {
      indices_3d.push(k);
      indices_3d.push(j);
    } else {
      indices_3d.push(j);
      indices_3d.push(k);
    }
  };

  // Extrude vertices
  for point in &buffers.vertices {
    vertices_3d.push(Point3D::new(point.x, point.y, 0.0));
  }
  for point in buffers.vertices {
    vertices_3d.push(Point3D::new(point.x, point.y, height));
  }

  // Extrude faces and count edges
  let mut edge_counts: FnvHashMap<(u32, u32), (usize, bool)> = FnvHashMap::default();
  for [i, j, k] in buffers.indices.iter().copied().const_chunks::<3>() {
    // Original face
    add_face(i, j, k, false);
    // Extruded face
    add_face(
      vertex_count_2d + i,
      vertex_count_2d + j,
      vertex_count_2d + k,
      true,
    );

    let edges = [(i, j), (j, k), (k, i)];
    for edge in edges {
      let min_val = std::cmp::min(edge.0, edge.1);
      let max_val = std::cmp::max(edge.0, edge.1);
      let ordered_edge = (min_val, max_val);
      let flipped = ordered_edge.0 != edge.0;
      let entry = edge_counts.entry(ordered_edge).or_insert((0, flipped));
      entry.0 += 1;
    }
  }

  // Create side faces for boundary edges only
  for (edge, &(count, flipped)) in edge_counts.iter() {
    if count == 1 {
      add_face(edge.0, edge.1, vertex_count_2d + edge.1, !flipped);
      add_face(
        edge.0,
        vertex_count_2d + edge.1,
        vertex_count_2d + edge.0,
        !flipped,
      );
    }
  }

  VertexBuffers {
    vertices: vertices_3d,
    indices: indices_3d,
  }
}

fn build_rune_mesh_3d(params: &RuneGenParams) -> VertexBuffers<Point3D<f32, UnknownUnit>, u32> {
  let buffers = build_and_tessellate_path(params);
  extrude_to_3d(buffers, params.extrude_height)
}
