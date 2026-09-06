use std::rc::Rc;

use super::{
  segment::*,
  subpath::{LastCtrl, Subpath},
  FillRule, Path, PathKind,
};
use crate::{ErrorStack, Vec2};

#[derive(Clone, Debug)]
pub enum DrawCommand {
  MoveTo(Vec2),
  LineTo(Vec2),
  QuadraticBezier {
    ctrl: Vec2,
    to: Vec2,
  },
  SmoothQuadraticBezier {
    to: Vec2,
  },
  CubicBezier {
    ctrl1: Vec2,
    ctrl2: Vec2,
    to: Vec2,
  },
  SmoothCubicBezier {
    ctrl2: Vec2,
    to: Vec2,
  },
  Arc {
    rx: f32,
    ry: f32,
    x_axis_rotation: f32,
    large_arc: bool,
    sweep: bool,
    to: Vec2,
  },
  Close,
}

struct OpenLeaf {
  start: Vec2,
  current: Vec2,
  segments: Vec<PathSegment>,
  anchors: Vec<bool>,
  last_ctrl: Option<LastCtrl>,
}

impl OpenLeaf {
  fn new(start: Vec2) -> Self {
    Self {
      start,
      current: start,
      segments: Vec::new(),
      anchors: Vec::new(),
      last_ctrl: None,
    }
  }

  fn from_subpath(sp: &Subpath) -> Self {
    Self {
      start: sp.start,
      current: sp.end(),
      segments: sp.segments.clone(),
      anchors: sp.anchors.clone(),
      last_ctrl: sp.last_ctrl,
    }
  }

  fn push(&mut self, seg: Option<PathSegment>, to: Vec2, ctrl: Option<LastCtrl>) {
    if let Some(seg) = seg.filter(|s| s.length() > LENGTH_EPSILON) {
      self.segments.push(seg);
      self.anchors.push(true);
    }
    self.current = to;
    self.last_ctrl = ctrl;
  }

  fn into_subpath(self, closed: bool) -> Subpath {
    Subpath::new(
      self.start,
      self.segments,
      closed,
      self.anchors,
      false,
      self.last_ctrl,
    )
  }
}

/// Mutable pen over a concrete tree: shared closed/earlier leaves plus one owned open tail.
pub(crate) struct PathBuilder {
  leaves: Vec<Rc<Path>>,
  open: Option<OpenLeaf>,
}

impl PathBuilder {
  pub(crate) fn new() -> Self {
    Self {
      leaves: Vec::new(),
      open: None,
    }
  }

  pub(crate) fn from_path(path: &Rc<Path>, op: &str) -> Result<Self, ErrorStack> {
    if let Some(lazy) = path.first_lazy_name() {
      return Err(ErrorStack::new(format!(
        "`{op}`: path has no draw commands (contains `{lazy}`); use `discretize_path` first"
      )));
    }
    let mut leaves = match &path.kind {
      PathKind::Subpath(_) => vec![Rc::clone(path)],
      PathKind::Group(g) => g.children.clone(),
      PathKind::Abstract(_) => unreachable!(),
    };
    let open = match leaves.last().map(|l| &l.kind) {
      Some(PathKind::Subpath(sp)) if !sp.closed => Some(OpenLeaf::from_subpath(sp)),
      _ => None,
    };
    if open.is_some() {
      leaves.pop();
    }
    Ok(Self { leaves, open })
  }

  fn current_point(&self) -> Vec2 {
    if let Some(o) = &self.open {
      return o.current;
    }
    match self.leaves.last().map(|l| &l.kind) {
      Some(PathKind::Subpath(sp)) => {
        if sp.closed {
          sp.start
        } else {
          sp.end()
        }
      }
      _ => Vec2::zeros(),
    }
  }

  fn open_leaf(&mut self) -> &mut OpenLeaf {
    if self.open.is_none() {
      self.open = Some(OpenLeaf::new(self.current_point()));
    }
    self.open.as_mut().unwrap()
  }

  fn flush_open(&mut self, closed: bool) {
    if let Some(o) = self.open.take() {
      self
        .leaves
        .push(Rc::new(Path::leaf(o.into_subpath(closed))));
    }
  }

  pub(crate) fn apply(&mut self, cmd: DrawCommand) {
    match cmd {
      DrawCommand::MoveTo(p) => {
        self.flush_open(false);
        self.open = Some(OpenLeaf::new(p));
      }
      DrawCommand::LineTo(to) => {
        let o = self.open_leaf();
        o.push(Some(line_segment(o.current, to)), to, None);
      }
      DrawCommand::QuadraticBezier { ctrl, to } => {
        let o = self.open_leaf();
        o.push(
          Some(quad_segment(o.current, ctrl, to)),
          to,
          Some(LastCtrl::Quad(ctrl)),
        );
      }
      DrawCommand::SmoothQuadraticBezier { to } => {
        let o = self.open_leaf();
        let start = o.current;
        let ctrl = match o.last_ctrl {
          Some(LastCtrl::Quad(c)) => start + (start - c),
          _ => start,
        };
        o.push(
          Some(quad_segment(start, ctrl, to)),
          to,
          Some(LastCtrl::Quad(ctrl)),
        );
      }
      DrawCommand::CubicBezier { ctrl1, ctrl2, to } => {
        let o = self.open_leaf();
        o.push(
          Some(cubic_segment(o.current, ctrl1, ctrl2, to)),
          to,
          Some(LastCtrl::Cubic(ctrl2)),
        );
      }
      DrawCommand::SmoothCubicBezier { ctrl2, to } => {
        let o = self.open_leaf();
        let start = o.current;
        let ctrl1 = match o.last_ctrl {
          Some(LastCtrl::Cubic(c)) => start + (start - c),
          _ => start,
        };
        o.push(
          Some(cubic_segment(start, ctrl1, ctrl2, to)),
          to,
          Some(LastCtrl::Cubic(ctrl2)),
        );
      }
      DrawCommand::Arc {
        rx,
        ry,
        x_axis_rotation,
        large_arc,
        sweep,
        to,
      } => {
        let o = self.open_leaf();
        let seg = build_arc_segment(o.current, to, rx, ry, x_axis_rotation, large_arc, sweep);
        o.push(seg, to, None);
      }
      DrawCommand::Close => self.close_open(),
    }
  }

  pub(crate) fn close_open(&mut self) {
    let Some(o) = self.open.as_mut() else {
      return;
    };
    if o.segments.is_empty() {
      return;
    }
    let (cur, start) = (o.current, o.start);
    o.push(Some(line_segment(cur, start)), start, None);
    self.flush_open(true);
  }

  pub(crate) fn close_all(&mut self) {
    self.close_open();
    for leaf in &mut self.leaves {
      if let PathKind::Subpath(sp) = &leaf.kind {
        if !sp.closed && !sp.segments.is_empty() {
          *leaf = Rc::new(Path::leaf(sp.closed_copy()));
        }
      }
    }
  }

  pub(crate) fn finish(mut self, fill_rule: Option<FillRule>) -> Path {
    self.flush_open(false);
    Path::concrete_group(self.leaves, fill_rule)
  }
}

fn line_run(points: &[Vec2], closed: bool) -> (Vec<PathSegment>, Vec<bool>) {
  let n = points.len();
  let count = if closed { n } else { n.saturating_sub(1) };
  let mut segments = Vec::with_capacity(count);
  for i in 0..count {
    let seg = line_segment(points[i], points[(i + 1) % n]);
    if seg.length() > LENGTH_EPSILON {
      segments.push(seg);
    }
  }
  let anchors = vec![true; segments.len()];
  (segments, anchors)
}

pub(crate) fn circle_subpath(center: Vec2, radius: f32, reversed: bool) -> Subpath {
  let right = center + Vec2::new(radius, 0.0);
  let left = center - Vec2::new(radius, 0.0);
  let sweep = !reversed;
  let segments: Vec<_> = [
    build_arc_segment(right, left, radius, radius, 0.0, false, sweep),
    build_arc_segment(left, right, radius, radius, 0.0, false, sweep),
  ]
  .into_iter()
  .flatten()
  .filter(|s| s.length() > LENGTH_EPSILON)
  .collect();
  let anchors = vec![true; segments.len()];
  Subpath::new(right, segments, true, anchors, false, None)
}

pub(crate) fn rect_subpath(center: Vec2, width: f32, height: f32, reversed: bool) -> Subpath {
  let (hw, hh) = (width * 0.5, height * 0.5);
  let tr = Vec2::new(center.x + hw, center.y + hh);
  let tl = Vec2::new(center.x - hw, center.y + hh);
  let bl = Vec2::new(center.x - hw, center.y - hh);
  let br = Vec2::new(center.x + hw, center.y - hh);
  let corners = if reversed {
    [tr, br, bl, tl]
  } else {
    [tr, tl, bl, br]
  };
  let (segments, anchors) = line_run(&corners, true);
  Subpath::new(tr, segments, true, anchors, false, None)
}

pub(crate) fn polygon_subpath(points: &[Vec2]) -> Result<Subpath, ErrorStack> {
  let (segments, anchors) = line_run(points, true);
  if segments.len() < 3 {
    return Err(ErrorStack::new(format!(
      "`polygon`: expected at least 3 distinct points, found {}",
      points.len()
    )));
  }
  Ok(Subpath::new(
    segments[0].start_point(),
    segments,
    true,
    anchors,
    false,
    None,
  ))
}

pub(crate) fn polyline_subpath(points: &[Vec2]) -> Result<Subpath, ErrorStack> {
  if points.len() < 2 {
    return Err(ErrorStack::new(format!(
      "`polyline`: expected at least 2 points, found {}",
      points.len()
    )));
  }
  let (segments, anchors) = line_run(points, false);
  Ok(Subpath::new(
    points[0], segments, false, anchors, false, None,
  ))
}

impl Path {
  pub(crate) fn from_draw_commands(
    cmds: impl IntoIterator<Item = DrawCommand>,
    close_all: bool,
  ) -> Path {
    let mut b = PathBuilder::new();
    for cmd in cmds {
      b.apply(cmd);
    }
    if close_all {
      b.close_all();
    }
    b.finish(None)
  }

  /// Polyline leaves. With `anchors` (one flag per vertex, parallel to `polylines`) the
  /// leaves are producer-marked; otherwise every vertex is an authored joint. Closed polylines
  /// drop a duplicated trailing vertex; sub-2-point polylines are skipped.
  pub(crate) fn from_polylines(
    polylines: Vec<(Vec<Vec2>, bool)>,
    anchors: Option<Vec<Vec<bool>>>,
  ) -> Path {
    let explicit = anchors.is_some();
    let mut leaves = Vec::with_capacity(polylines.len());
    for (ix, (mut points, closed)) in polylines.into_iter().enumerate() {
      let mut flags = anchors
        .as_ref()
        .map(|a| a[ix].clone())
        .unwrap_or_else(|| vec![true; points.len()]);
      if points.len() < 2 {
        continue;
      }
      if closed && (points[0] - points[points.len() - 1]).norm() <= 1e-6 {
        points.pop();
        flags.pop();
      }
      let n = points.len();
      let count = if closed { n } else { n - 1 };
      let (mut segments, mut seg_anchors) = (Vec::with_capacity(count), Vec::with_capacity(count));
      let mut pending = false;
      for i in 0..count {
        let seg = line_segment(points[i], points[(i + 1) % n]);
        if seg.length() > LENGTH_EPSILON {
          segments.push(seg);
          seg_anchors.push(flags[i] || pending);
          pending = false;
        } else {
          pending |= flags[i];
        }
      }
      if closed && pending && !seg_anchors.is_empty() {
        seg_anchors[0] = true;
      }
      leaves.push(Rc::new(Path::leaf(Subpath::new(
        points[0],
        segments,
        closed,
        seg_anchors,
        explicit,
        None,
      ))));
    }
    Path::concrete_group(leaves, None)
  }

  pub(crate) fn pen(self: &Rc<Path>, cmd: DrawCommand, op: &str) -> Result<Path, ErrorStack> {
    let mut b = PathBuilder::from_path(self, op)?;
    b.apply(cmd);
    Ok(b.finish(self.fill_rule))
  }

  pub(crate) fn close_all(self: &Rc<Path>) -> Result<Path, ErrorStack> {
    let mut b = PathBuilder::from_path(self, "close_all")?;
    b.close_all();
    Ok(b.finish(self.fill_rule))
  }
}
