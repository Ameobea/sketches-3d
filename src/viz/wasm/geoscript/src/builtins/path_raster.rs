//! `rasterize_path` / `path_sdf` / `path_uv`: 2D paths into textures via `raster2d`. Path space
//! is the texture's [0,1]² UV space; texel `(i, j)` samples `((i+.5)/w, (j+.5)/h)`.

use std::rc::Rc;

use fxhash::FxHashMap;

use super::texture::MAX_TEXTURE_DIM;
use super::trace_path::{as_path_sampler, as_path_tracer, sample_path_subpaths, FillRule};
use crate::builtins::resolve_tile_period;
use crate::raster2d::{replicate_tiled, EdgeList, Segment, SegmentField, MAX_TILED_POINTS};
use crate::{
  ArgRef, ErrorStack, EvalCtx, Mat4, Sym, TexStorage, TextureHandle, TextureWrap, Value, Vec2,
};

const BLACK_BOX_SAMPLES: usize = 512;
/// Default flattening when `curve_angle_degrees` is nil: chords within this many texels of the
/// curve (segment counts scale with sqrt(radius) instead of the ambient 1 degree angle), and
/// never coarser than `SAGITTA_FLOOR_ANGLE_DEGREES` per chord.
const MAX_SAGITTA_TEXELS: f32 = 0.05;
const SAGITTA_FLOOR_ANGLE_DEGREES: f32 = 12.;

struct ArgIx {
  fill_rule: Option<usize>,
  tileable: usize,
  wrap: usize,
  curve_angle: usize,
}

struct RasterInput {
  subpaths: Vec<(Vec<Vec2>, bool)>,
  /// Source subpaths plus their periodic copies when tiling; each tagged with its source index.
  tiled: Vec<(Vec<Vec2>, bool, u32)>,
  t_spans: Vec<(f32, f32)>,
  fill_rule: FillRule,
  wrap: TextureWrap,
  w: usize,
  h: usize,
}

fn length_spans(polys: &[(Vec<Vec2>, bool)]) -> Vec<(f32, f32)> {
  let lens: Vec<f32> = polys
    .iter()
    .map(|(pts, closed)| {
      let mut acc = pts.windows(2).map(|s| (s[1] - s[0]).norm()).sum::<f32>();
      if *closed && pts.len() >= 3 {
        acc += (pts[0] - pts[pts.len() - 1]).norm();
      }
      acc
    })
    .collect();
  let total: f32 = lens.iter().sum();
  let mut spans = Vec::with_capacity(lens.len());
  let mut acc = 0.;
  for len in lens {
    let t0 = if total > 0. { acc / total } else { 0. };
    acc += len;
    spans.push((t0, if total > 0. { acc / total } else { 0. }));
  }
  spans
}

impl RasterInput {
  fn from_args(
    ctx: &EvalCtx,
    arg_refs: &[ArgRef],
    args: &[Value],
    kwargs: &FxHashMap<Sym, Value>,
    ix: ArgIx,
    fn_name: &str,
  ) -> Result<Self, ErrorStack> {
    let path_val = arg_refs[0].resolve(args, kwargs);
    let cb = path_val.as_callable().ok_or_else(|| {
      ErrorStack::new(format!(
        "Invalid `path` argument for `{fn_name}`; expected Callable, found: {path_val:?}"
      ))
    })?;
    let w = arg_refs[1].resolve(args, kwargs).as_int().unwrap();
    let h = arg_refs[2].resolve(args, kwargs).as_int().unwrap();
    if w < 1 || h < 1 || w > MAX_TEXTURE_DIM || h > MAX_TEXTURE_DIM {
      return Err(ErrorStack::new(format!(
        "Invalid `{fn_name}` dims {w}x{h}; expected 1..={MAX_TEXTURE_DIM} per side"
      )));
    }
    let wrap = TextureWrap::from_name(arg_refs[ix.wrap].resolve(args, kwargs).as_str().unwrap())?;
    let period = resolve_tile_period(arg_refs[ix.tileable].resolve(args, kwargs))?;
    let angle_val = arg_refs[ix.curve_angle].resolve(args, kwargs);
    let angle = ctx.resolve_curve_angle_degrees(angle_val);
    if angle <= 0. {
      return Err(ErrorStack::new(format!(
        "Invalid curve_angle_degrees for `{fn_name}`; expected > 0, found: {angle}"
      )));
    }
    let (flat_angle, max_sagitta) = if matches!(angle_val, Value::Nil) {
      (
        SAGITTA_FLOOR_ANGLE_DEGREES.to_radians(),
        MAX_SAGITTA_TEXELS / w.max(h) as f32,
      )
    } else {
      (angle.to_radians(), f32::INFINITY)
    };
    let sampler = as_path_sampler(cb);
    let fill_rule = match ix.fill_rule.map(|i| arg_refs[i].resolve(args, kwargs)) {
      Some(v) if !matches!(v, Value::Nil) => FillRule::parse(v, fn_name)?,
      _ => sampler
        .and_then(|s| s.fill_rule())
        .unwrap_or(FillRule::NonZero),
    };

    let (subpaths, t_spans): (Vec<_>, Vec<_>) =
      match sampler.and_then(|s| s.sample_subpaths_flat(flat_angle, max_sagitta)) {
        Some(raw) => {
          let mut spans = sampler
            .unwrap()
            .subpath_t_spans()
            .filter(|s| s.len() == raw.len())
            .unwrap_or_else(|| length_spans(&raw));
          // `sample_subpaths` keeps subpath order under `reverse`; `subpath_t_spans` flips it.
          if as_path_tracer(cb).is_some_and(|t| t.reverse) {
            spans.reverse();
          }
          raw
            .into_iter()
            .zip(spans)
            .filter(|((pts, _), _)| pts.len() >= 2)
            .unzip()
        }
        None => {
          let polys = sample_path_subpaths(
            ctx,
            cb,
            angle.to_radians(),
            BLACK_BOX_SAMPLES,
            None,
            fn_name,
          )?;
          let spans = length_spans(&polys);
          (polys, spans)
        }
      };
    if subpaths.is_empty() {
      return Err(ErrorStack::new(format!(
        "`{fn_name}`: path contains no drawable segments"
      )));
    }
    let tiled = match period {
      Some(p) => replicate_tiled(&subpaths, p).map_err(|n| {
        ErrorStack::new(format!(
          "`{fn_name}`: tiling with period {p} needs {n}+ polyline points (max \
           {MAX_TILED_POINTS}); use a larger period or a simpler path"
        ))
      })?,
      None => subpaths
        .iter()
        .enumerate()
        .map(|(i, (pts, closed))| (pts.clone(), *closed, i as u32))
        .collect(),
    };
    Ok(RasterInput {
      subpaths,
      tiled,
      t_spans,
      fill_rule,
      wrap,
      w: w as usize,
      h: h as usize,
    })
  }

  fn texel_polylines(&self) -> Vec<(Vec<Vec2>, bool)> {
    let (w, h) = (self.w as f32, self.h as f32);
    self
      .tiled
      .iter()
      .map(|(pts, closed, _)| {
        (
          pts.iter().map(|p| Vec2::new(p.x * w, p.y * h)).collect(),
          *closed,
        )
      })
      .collect()
  }

  /// Path-space segments (tiled copies included, tagged by source subpath) plus per-source
  /// polyline length and inward-normal flip (CW closed).
  fn segments(&self) -> (Vec<Segment>, Vec<f32>, Vec<bool>) {
    let (mut lens, mut flips) = (Vec::new(), Vec::new());
    for (pts, closed) in &self.subpaths {
      let n = pts.len();
      let wrap = *closed && n >= 3;
      let (mut acc, mut area2) = (0., 0.);
      for i in 0..if wrap { n } else { n - 1 } {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        acc += (b - a).norm();
        area2 += a.x * b.y - b.x * a.y;
      }
      lens.push(acc);
      flips.push(wrap && area2 < 0.);
    }
    let mut segs = Vec::new();
    for (pts, closed, si) in &self.tiled {
      let n = pts.len();
      let mut acc = 0.;
      for i in 0..if *closed && n >= 3 { n } else { n - 1 } {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        segs.push(Segment {
          a,
          b,
          subpath: *si,
          len_before: acc,
        });
        acc += (b - a).norm();
      }
    }
    (segs, lens, flips)
  }

  fn field(&self, segs: Vec<Segment>) -> SegmentField {
    SegmentField::new(segs, self.w, self.h)
  }

  fn texture(&self, planes: Vec<Vec<f32>>) -> Value {
    Value::Texture(Rc::new(TextureHandle {
      channels: planes.len(),
      storage: TexStorage::from_plane_vecs(planes),
      width: self.w,
      height: self.h,
      wrap: self.wrap,
      min_filter: None,
      mag_filter: None,
      format: None,
      transform: Mat4::identity(),
      mips: Default::default(),
    }))
  }
}

const FILL_ARGS: ArgIx = ArgIx {
  fill_rule: Some(3),
  tileable: 4,
  wrap: 5,
  curve_angle: 6,
};

pub(crate) fn rasterize_path_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let input = RasterInput::from_args(ctx, arg_refs, args, kwargs, FILL_ARGS, "rasterize_path")?;
  let cov =
    EdgeList::new(&input.texel_polylines(), true, input.w, input.h).coverage(input.fill_rule);
  Ok(input.texture(vec![cov]))
}

pub(crate) fn path_sdf_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let input = RasterInput::from_args(ctx, arg_refs, args, kwargs, FILL_ARGS, "path_sdf")?;
  let (segs, _, _) = input.segments();
  let hits = input.field(segs).nearest_at_centers();
  let wn = EdgeList::new(&input.texel_polylines(), false, input.w, input.h).winding_at_centers();
  let sd = hits
    .iter()
    .zip(&wn)
    .map(|(hit, &wn)| {
      if input.fill_rule.accepts(wn) {
        -hit.dist
      } else {
        hit.dist
      }
    })
    .collect();
  Ok(input.texture(vec![sd]))
}

pub(crate) fn path_uv_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let ix = ArgIx {
    fill_rule: None,
    tileable: 3,
    wrap: 4,
    curve_angle: 5,
  };
  let input = RasterInput::from_args(ctx, arg_refs, args, kwargs, ix, "path_uv")?;
  let (w, h) = (input.w, input.h);
  let (segs, lens, flips) = input.segments();
  let field = input.field(segs);
  let hits = field.nearest_at_centers();
  let segs = field.segments();
  let seg_lens: Vec<f32> = segs.iter().map(Segment::len).collect();
  let (mut t, mut n) = (Vec::with_capacity(w * h), Vec::with_capacity(w * h));
  for (ix, hit) in hits.iter().enumerate() {
    let seg = &segs[hit.seg as usize];
    let sp = seg.subpath as usize;
    let (t0, t1) = input.t_spans[sp];
    let frac = if lens[sp] > 0. {
      (seg.len_before + hit.s * seg_lens[hit.seg as usize]) / lens[sp]
    } else {
      0.
    };
    t.push(t0 + (t1 - t0) * frac);
    let p = Vec2::new(
      ((ix % w) as f32 + 0.5) / w as f32,
      ((ix / w) as f32 + 0.5) / h as f32,
    );
    let sign = seg.side(p) * if flips[sp] { -1. } else { 1. };
    n.push(hit.dist * sign);
  }
  Ok(input.texture(vec![t, n]))
}

pub(crate) fn fit_path_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let path_val = arg_refs[0].resolve(args, kwargs);
  let cb = path_val.as_callable().ok_or_else(|| {
    ErrorStack::new(format!(
      "Invalid `path` argument for `fit_path`; expected Callable, found: {path_val:?}"
    ))
  })?;
  let pad = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
  if !(0. ..0.5).contains(&pad) {
    return Err(ErrorStack::new(format!(
      "`fit_path` pad must be in [0, 0.5); found {pad}"
    )));
  }
  let analytic = as_path_tracer(cb).and_then(|t| t.analytic_aabb().ok().flatten());
  let (mins, maxs) = match analytic {
    Some(b) => b,
    None => {
      let angle = ctx.resolve_curve_angle_degrees(&Value::Nil).to_radians();
      let polys = sample_path_subpaths(ctx, cb, angle, BLACK_BOX_SAMPLES, None, "fit_path")?;
      let pts = polys.iter().flat_map(|(pts, _)| pts.iter());
      let mut lo = Vec2::repeat(f32::INFINITY);
      let mut hi = Vec2::repeat(f32::NEG_INFINITY);
      for p in pts {
        lo = lo.inf(p);
        hi = hi.sup(p);
      }
      (lo, hi)
    }
  };
  let size = maxs - mins;
  let extent = size.x.max(size.y);
  if !(extent > 0.) {
    return Err(ErrorStack::new(
      "`fit_path`: path has a degenerate bounding box",
    ));
  }
  let s = (1. - 2. * pad) / extent;
  let off = (Vec2::repeat(1. - 2. * pad) - size * s) * 0.5 + Vec2::repeat(pad) - mins * s;
  let m = nalgebra::Matrix3::new(s, 0., off.x, 0., s, off.y, 0., 0., 1.);
  super::apply_path_transform(cb, m)
}

#[cfg(test)]
mod tests {
  use std::rc::Rc;

  use crate::{parse_and_eval_program, EvalCtx, TextureHandle, Value};

  fn get_tex(ctx: &EvalCtx, name: &str) -> Rc<TextureHandle> {
    match ctx.get_global(name).unwrap() {
      Value::Texture(t) => t,
      other => panic!("Expected {name} to be a texture, found: {other:?}"),
    }
  }

  fn px(t: &TextureHandle, x: usize, y: usize, c: usize) -> f32 {
    t.as_interleaved()[(y * t.width + x) * t.channels + c]
  }

  #[test]
  fn coverage_area_fill_rules_and_implicit_close() {
    let ctx = parse_and_eval_program(
      r#"
sq = build_path(path { rect(vec2(0.5, 0.5), 0.5) })
sq_cov = rasterize_path(sq, width=16, height=16)
circ = build_path(path { circle(vec2(0.5, 0.5), 0.25) })
circ_cov = rasterize_path(circ, width=128, height=128)
ring = build_path(path { rect(vec2(0.5, 0.5), 0.5) circle(vec2(0.5, 0.5), 0.125) })
ring_nz = rasterize_path(ring, width=16, height=16)
ring_eo = rasterize_path(ring, width=16, height=16, fill_rule="evenodd")
ring_hole = build_path(path { rect(vec2(0.5, 0.5), 0.5) circle(vec2(0.5, 0.5), 0.125) | reverse })
ring_hole_nz = rasterize_path(ring_hole, width=16, height=16)
tri = build_path(path { move(0, 0) line(1, 0) line(0, 1) })
tri_cov = rasterize_path(tri, width=16, height=16)
tri_sd = path_sdf(tri, width=16, height=16)
tiny = build_path(path { circle(vec2(0.5, 0.5), 0.03) })
tiny_cov = rasterize_path(tiny, width=256, height=256)
"#,
    )
    .unwrap();

    let sq = get_tex(&ctx, "sq_cov");
    assert_eq!(sq.channels, 1);
    let sum: f32 = sq.as_interleaved().iter().sum();
    assert!((sum - 64.).abs() < 1e-3, "{sum}");
    assert_eq!(px(&sq, 8, 8, 0), 1.);
    assert_eq!(px(&sq, 0, 0, 0), 0.);

    let circ = get_tex(&ctx, "circ_cov");
    let sum: f32 = circ.as_interleaved().iter().sum();
    let expected = std::f32::consts::PI * 32. * 32.;
    assert!(
      (sum - expected).abs() / expected < 0.005,
      "{sum} vs {expected}"
    );

    assert_eq!(px(&get_tex(&ctx, "ring_nz"), 8, 8, 0), 1.);
    assert_eq!(px(&get_tex(&ctx, "ring_eo"), 8, 8, 0), 0.);
    assert_eq!(px(&get_tex(&ctx, "ring_hole_nz"), 8, 8, 0), 0.);
    assert_eq!(px(&get_tex(&ctx, "ring_hole_nz"), 5, 5, 0), 1.);

    let tri = get_tex(&ctx, "tri_cov");
    let sum: f32 = tri.as_interleaved().iter().sum();
    assert!((sum - 128.).abs() < 0.5, "{sum}");
    let sd_min = get_tex(&ctx, "tri_sd")
      .as_interleaved()
      .iter()
      .cloned()
      .fold(f32::INFINITY, f32::min);
    assert!(sd_min >= 0., "open subpath must be unsigned, got {sd_min}");

    // Small arcs must survive the arc builder (radius well below the old absolute epsilon).
    let sum: f32 = get_tex(&ctx, "tiny_cov").as_interleaved().iter().sum();
    let expected = std::f32::consts::PI * (0.03f32 * 256.).powi(2);
    assert!(
      (sum - expected).abs() / expected < 0.01,
      "{sum} vs {expected}"
    );
  }

  #[test]
  fn sdf_matches_analytic_circle_and_offset_path_threshold() {
    let ctx = parse_and_eval_program(
      r#"
circ = build_path(path { circle(vec2(0.5, 0.5), 0.25) })
sd = path_sdf(circ, width=64, height=32)
"#,
    )
    .unwrap();
    let sd = get_tex(&ctx, "sd");
    for y in 0..32 {
      for x in 0..64 {
        let (u, v) = ((x as f32 + 0.5) / 64., (y as f32 + 0.5) / 32.);
        let expected = ((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt() - 0.25;
        let got = px(&sd, x, y, 0);
        assert!(
          (got - expected).abs() < 2e-3,
          "({x},{y}) {got} vs {expected}"
        );
      }
    }
  }

  #[test]
  fn uv_along_and_across_with_reverse_and_inward_flip() {
    let ctx = parse_and_eval_program(
      r#"
line = build_path(path { move(0, 0.5) line(1, 0.5) })
uv = path_uv(line, width=32, height=16)
uv_rev = path_uv(build_path(path { move(0, 0.5) line(1, 0.5) }, reverse=true), width=32, height=16)
cw_sq = build_path(path { rect(vec2(0.5, 0.5), 0.5) | reverse })
uv_cw = path_uv(cw_sq, width=16, height=16)
ccw_sq = build_path(path { rect(vec2(0.5, 0.5), 0.5) })
uv_ccw = path_uv(ccw_sq, width=16, height=16)
"#,
    )
    .unwrap();
    let uv = get_tex(&ctx, "uv");
    assert_eq!(uv.channels, 2);
    for x in 0..32 {
      let u = (x as f32 + 0.5) / 32.;
      assert!((px(&uv, x, 3, 0) - u).abs() < 1e-5);
      assert!((px(&uv, x, 12, 0) - u).abs() < 1e-5);
      // Travel is +x, so +y is left: positive n above the line, negative below.
      assert!((px(&uv, x, 12, 1) - (12.5 / 16. - 0.5)).abs() < 1e-5);
      assert!((px(&uv, x, 3, 1) - (3.5 / 16. - 0.5)).abs() < 1e-5);
    }
    let uv_rev = get_tex(&ctx, "uv_rev");
    assert!((px(&uv_rev, 4, 3, 0) - (1. - 4.5 / 32.)).abs() < 1e-5);
    assert!(px(&uv_rev, 4, 12, 1) < 0.);

    // Inside a closed subpath n is positive regardless of winding (path_frame convention).
    assert!(px(&get_tex(&ctx, "uv_cw"), 8, 8, 1) > 0.);
    assert!(px(&get_tex(&ctx, "uv_ccw"), 8, 8, 1) > 0.);
    assert!(px(&get_tex(&ctx, "uv_ccw"), 0, 8, 1) < 0.);
  }

  #[test]
  fn tiling_wraps_coverage_and_distance() {
    let ctx = parse_and_eval_program(
      r#"
dot = build_path(path { circle(vec2(0, 0), 0.25) })
cov = rasterize_path(dot, width=64, height=64, tileable=true)
cov_half = rasterize_path(dot, width=64, height=64, tileable=0.5)
sd = path_sdf(dot, width=64, height=64, tileable=true)
uv = path_uv(dot, width=64, height=64, tileable=true)
"#,
    )
    .unwrap();
    let cov = get_tex(&ctx, "cov");
    let sum: f32 = cov.as_interleaved().iter().sum();
    let disc = std::f32::consts::PI * 16. * 16.;
    assert!((sum - disc).abs() / disc < 0.005, "{sum} vs {disc}");
    for y in 0..64 {
      for x in 0..64 {
        let v = px(&cov, x, y, 0);
        assert!((v - px(&cov, 63 - x, y, 0)).abs() < 1e-4);
        assert!((v - px(&cov, x, 63 - y, 0)).abs() < 1e-4);
      }
    }
    let sum: f32 = get_tex(&ctx, "cov_half").as_interleaved().iter().sum();
    assert!((sum - 4. * disc).abs() / (4. * disc) < 0.005, "{sum}");

    let sd = get_tex(&ctx, "sd");
    let uv = get_tex(&ctx, "uv");
    for y in 0..64 {
      for x in 0..64 {
        let p = crate::Vec2::new((x as f32 + 0.5) / 64., (y as f32 + 0.5) / 64.);
        let mut expected = f32::INFINITY;
        for ky in -1..=1 {
          for kx in -1..=1 {
            expected = expected.min((p - crate::Vec2::new(kx as f32, ky as f32)).norm() - 0.25);
          }
        }
        assert!((px(&sd, x, y, 0) - expected).abs() < 2e-3, "({x},{y})");
        assert!((px(&uv, x, y, 1) + expected).abs() < 2e-3, "({x},{y})");
      }
    }
  }

  #[test]
  fn fit_path_scales_aabb_into_padded_unit_square() {
    let ctx = parse_and_eval_program(
      r#"
wide = build_path(path { rect(vec2(3, 7), vec2(6, 2)) })
fitted = fit_path(wide, pad=0.1)
bb = path_aabb(fitted)
lo = bb[0]
hi = bb[1]
"#,
    )
    .unwrap();
    let lo = ctx.get_global("lo").unwrap();
    let hi = ctx.get_global("hi").unwrap();
    let (lo, hi) = (lo.as_vec2().unwrap(), hi.as_vec2().unwrap());
    assert!(
      (lo.x - 0.1).abs() < 1e-5 && (hi.x - 0.9).abs() < 1e-5,
      "{lo:?} {hi:?}"
    );
    let expected_h = 0.8 / 3.;
    assert!((hi.y - lo.y - expected_h).abs() < 1e-5);
    assert!((lo.y - (0.5 - expected_h / 2.)).abs() < 1e-5, "{lo:?}");
  }
}
