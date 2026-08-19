use std::rc::Rc;

use crate::{Mat4, TexStorage, TextureHandle, TextureWrap};

fn tex(w: usize, h: usize, ch: usize) -> TextureHandle {
  let px: Vec<f32> = (0..w * h * ch).map(|i| i as f32).collect();
  TextureHandle {
    storage: TexStorage::Dense(Rc::new(px)),
    width: w,
    height: h,
    channels: ch,
    wrap: TextureWrap::Repeat,
    min_filter: None,
    mag_filter: None,
    format: None,
    transform: Mat4::identity(),
    mips: Default::default(),
  }
}

fn dense_of(t: &TextureHandle) -> Vec<f32> {
  t.as_dense().to_vec()
}

#[test]
fn as_dense_is_noop_for_dense() {
  let t = tex(4, 4, 3);
  assert_eq!(Rc::as_ptr(&t.as_dense()) as usize, t.base_ptr());
}

#[test]
fn crop_view_reads() {
  let t = tex(4, 3, 2);
  let c = t.crop_view(1, 1, 2, 2);
  assert!(!c.is_dense());
  assert_eq!((c.width, c.height, c.channels), (2, 2, 2));
  let mut expected = Vec::new();
  for y in 1..3 {
    for x in 1..3 {
      for ch in 0..2 {
        expected.push(((y * 4 + x) * 2 + ch) as f32);
      }
    }
  }
  assert_eq!(dense_of(&c), expected);
}

#[test]
fn swizzle_views_including_duplicates() {
  let t = tex(2, 2, 3);
  let bgr = t.swizzle_view(&[2, 1, 0]);
  assert_eq!(dense_of(&bgr)[0..3], [2., 1., 0.]);
  let rrr = t.swizzle_view(&[0, 0, 0]);
  assert_eq!(dense_of(&rrr)[0..3], [0., 0., 0.]);
  assert_eq!(dense_of(&rrr)[3..6], [3., 3., 3.]);
  let r = t.swizzle_view(&[1]);
  assert_eq!(r.channels, 1);
  assert_eq!(dense_of(&r), [1., 4., 7., 10.]);
}

#[test]
fn flip_views_and_involution() {
  let t = tex(3, 2, 1);
  let fx = t.flip_view(true, false);
  assert_eq!(dense_of(&fx), [2., 1., 0., 5., 4., 3.]);
  let fy = t.flip_view(false, true);
  assert_eq!(dense_of(&fy), [3., 4., 5., 0., 1., 2.]);
  assert_eq!(dense_of(&fx.flip_view(true, false)), dense_of(&t));
}

#[test]
fn view_composition_stays_flat() {
  let t = tex(4, 4, 3);
  let composed = t
    .flip_view(true, false)
    .crop_view(1, 1, 2, 2)
    .swizzle_view(&[2, 0])
    .swizzle_view(&[1, 1]);
  // Equivalent single-step reference: materialize each step separately.
  let step = TextureHandle {
    storage: TexStorage::Dense(t.flip_view(true, false).as_dense()),
    ..t.clone()
  }
  .crop_view(1, 1, 2, 2);
  let step = TextureHandle {
    storage: TexStorage::Dense(step.as_dense()),
    width: 2,
    height: 2,
    ..t.clone()
  }
  .swizzle_view(&[2, 0]);
  let step = TextureHandle {
    storage: TexStorage::Dense(step.as_dense()),
    width: 2,
    height: 2,
    channels: 2,
    ..t.clone()
  }
  .swizzle_view(&[1, 1]);
  assert_eq!(dense_of(&composed), dense_of(&step));
  // Composition flattened to a single view over the original dense base.
  assert_eq!(composed.base_ptr(), t.base_ptr());
}

#[test]
fn wrap_applies_in_view_space() {
  let t = tex(4, 4, 1);
  let c = t.crop_view(1, 1, 2, 2);
  // Repeat: a crop tiles the crop, not the base.
  assert_eq!(c.texel(-1, 0, 0), c.texel(1, 0, 0));
  assert_eq!(c.texel(2, 3, 0), c.texel(0, 1, 0));
  let clamped = TextureHandle {
    wrap: TextureWrap::Clamp,
    ..c.clone()
  };
  assert_eq!(clamped.texel(5, 0, 0), clamped.texel(1, 0, 0));
}

#[test]
fn dense_clone_materializes() {
  let t = tex(3, 3, 2);
  let v = t.flip_view(true, true).swizzle_view(&[1, 0]);
  let d = v.dense_clone();
  assert!(d.is_dense());
  assert_eq!(dense_of(&d), dense_of(&v));
  assert_ne!(d.base_ptr(), t.base_ptr());
}

#[test]
fn debug_marks_views() {
  let t = tex(4, 2, 1);
  assert!(!format!("{t:?}").contains("view"));
  assert!(format!("{:?}", t.crop_view(0, 0, 2, 1)).contains("view"));
}

mod geoscript_syntax {
  use std::rc::Rc;

  use crate::{parse_and_eval_program, TextureHandle, Value};

  fn eval(src: &str) -> crate::EvalCtx {
    parse_and_eval_program(src).unwrap()
  }

  fn get_tex(ctx: &crate::EvalCtx, name: &str) -> Rc<TextureHandle> {
    match ctx.get_global(name).unwrap() {
      Value::Texture(t) => t,
      other => panic!("Expected {name} to be a texture, found: {other:?}"),
    }
  }

  fn get_f(ctx: &crate::EvalCtx, name: &str) -> f32 {
    ctx.get_global(name).unwrap().as_float().unwrap()
  }

  const SETUP: &str = "
t = texture(4, 3, |uv| v3(uv.x, uv.y, uv.x + uv.y))
g = texture(4, 4, |uv| uv.x)
lut = texture(4, 1, |uv| uv.x)
";

  #[test]
  fn swizzle_fields() {
    let ctx = eval(&format!(
      "{SETUP}
r = t.r
bgr = t.bgr
rrr = t.rrr
xy = t.xy
"
    ));
    let t = get_tex(&ctx, "t");
    let td = t.as_dense();
    let r = get_tex(&ctx, "r");
    assert_eq!((r.channels, r.width, r.height), (1, 4, 3));
    assert!(!r.is_dense());
    assert_eq!(r.as_dense()[..4], [td[0], td[3], td[6], td[9]]);
    let bgr = get_tex(&ctx, "bgr");
    assert_eq!(bgr.as_dense()[..3], [td[2], td[1], td[0]]);
    let rrr = get_tex(&ctx, "rrr");
    assert_eq!(rrr.as_dense()[..3], [td[0], td[0], td[0]]);
    assert_eq!(get_tex(&ctx, "xy").channels, 2);

    let err = parse_and_eval_program("texture(2, 2, |uv| v2(uv.x, uv.y)).b").unwrap_err();
    assert!(err.to_string().contains("2-channel"), "{err}");
    let err = parse_and_eval_program("texture(2, 2, |uv| v4(0., 0., 0., 1.)).rgbaa").unwrap_err();
    assert!(err.to_string().contains("1 to 4"), "{err}");
  }

  #[test]
  fn indexing_forms_and_equivalences() {
    let ctx = eval(&format!(
      "{SETUP}
row = g[1]
px = g[1][2]
chan = t[1][2][1]
chan_swiz = t.g[1][2]
px_comma = g[1, 2]
crop2d = g[1..3, 0..2]
row_seg = g[1, 1..3]
col = g[0..2, 3]
lut_px = lut[2]
open_end = g[2..]
"
    ));
    let g = get_tex(&ctx, "g");
    let gd = g.as_dense();
    let row = get_tex(&ctx, "row");
    assert_eq!((row.width, row.height), (4, 1));
    assert_eq!(row.as_dense()[..], gd[4..8]);
    assert_eq!(get_f(&ctx, "px"), gd[6]);
    let t = get_tex(&ctx, "t");
    assert_eq!(get_f(&ctx, "chan"), t.texel_raw(2, 1, 1));
    assert_eq!(get_f(&ctx, "chan_swiz"), t.texel_raw(2, 1, 1));
    assert_eq!(get_f(&ctx, "px_comma"), gd[6]);
    let crop2d = get_tex(&ctx, "crop2d");
    assert_eq!((crop2d.width, crop2d.height), (2, 2));
    assert_eq!(crop2d.as_dense()[..], [gd[4], gd[5], gd[8], gd[9]]);
    let row_seg = get_tex(&ctx, "row_seg");
    assert_eq!((row_seg.width, row_seg.height), (2, 1));
    assert_eq!(row_seg.as_dense()[..], gd[5..7]);
    let col = get_tex(&ctx, "col");
    assert_eq!((col.width, col.height), (1, 2));
    assert_eq!(col.as_dense()[..], [gd[3], gd[7]]);
    assert_eq!(get_f(&ctx, "lut_px"), 2.5 / 4.);
    let open_end = get_tex(&ctx, "open_end");
    assert_eq!((open_end.width, open_end.height), (4, 2));
  }

  #[test]
  fn indexing_errors() {
    for (src, needle) in [
      ("texture(2, 2, |uv| 0.)[2]", "out of bounds"),
      ("texture(2, 2, |uv| 0.)[0 - 1]", "negative"),
      ("texture(2, 2, |uv| 0.)[0..3]", "out of bounds"),
      ("texture(2, 2, |uv| 0.)[1..1]", "empty"),
      ("texture(2, 2, |uv| 0.)[[0, 1]]", "not supported"),
      ("texture(2, 2, |uv| 0.)[1.5]", "must be ints or ranges"),
      ("[1, 2, 3][0, 1]", "only supported for textures"),
    ] {
      let err = parse_and_eval_program(src).unwrap_err();
      assert!(err.to_string().contains(needle), "{src}: {err}");
    }
  }

  #[test]
  fn flips_and_materialize() {
    let ctx = eval(&format!(
      "{SETUP}
fx = flip_x(g)
fy = g | flip_y
mat = materialize(fx)
"
    ));
    let g = get_tex(&ctx, "g");
    let gd = g.as_dense();
    let fx = get_tex(&ctx, "fx");
    assert!(!fx.is_dense());
    assert_eq!(fx.as_dense()[..4], [gd[3], gd[2], gd[1], gd[0]]);
    let fy = get_tex(&ctx, "fy");
    assert_eq!(fy.as_dense()[..4], gd[12..16]);
    let mat = get_tex(&ctx, "mat");
    assert!(mat.is_dense());
    assert_eq!(mat.as_dense()[..], fx.as_dense()[..]);
    // materialize on dense is identity
    let ctx = eval("g = texture(2, 2, |uv| uv.x)\nm = materialize(g)");
    assert_eq!(
      get_tex(&ctx, "g").base_ptr(),
      get_tex(&ctx, "m").base_ptr()
    );
  }

  /// op(view) == op(materialize(view)) across representative ops.
  #[test]
  fn op_equivalence_on_views() {
    let ctx = eval(
      "
base = texture(8, 8, |uv| v4(uv.x, uv.y, uv.x * uv.y, 1. - uv.x * 0.5))
view = flip_x(base[1..7, 2..8]).bgra
mv = materialize(view)
sum_v = view + view
sum_m = mv + mv
prod_v = view * 0.5
prod_m = mv * 0.5
blur_v = view | blur(1.5)
blur_m = mv | blur(1.5)
n_v = view.r | height_to_normal
n_m = mv.r | height_to_normal
map_v = view -> |val| val.x
map_m = mv -> |val| val.x
",
    );
    for (a, b) in [
      ("sum_v", "sum_m"),
      ("prod_v", "prod_m"),
      ("blur_v", "blur_m"),
      ("n_v", "n_m"),
      ("map_v", "map_m"),
    ] {
      let av = get_tex(&ctx, a).as_dense();
      let bv = get_tex(&ctx, b).as_dense();
      assert_eq!(av[..], bv[..], "{a} != {b}");
    }
  }

  #[test]
  fn operator_matrix_and_ufuncs() {
    let ctx = eval(
      "
g = texture(2, 2, |uv| uv.x)   // px: [0.25, 0.75, ...]
c = texture(2, 2, |uv| v3(uv.x, 0.5, 2.))
add_s = g + 1.
sub_s = g - 0.25
rsub_s = 1. - g
div_s = g / 2.
rdiv_s = 1. / g
sub_t = g - g
div_t = g / (g + 1.)
tint = c * v3(1., 2., 0.5)
tint2 = v3(1., 2., 0.5) * c
vadd = c + v3(1., 0., 0.)
vsub = v3(1., 1., 1.) - c
vdiv = c / v3(2., 2., 2.)
p = pow(g, 2.)
cl = clamp(0.3, 0.6, g)
ab = abs(0. - g)
ss = smoothstep(0., 1., g)
rm = g | remap(0., 1., 10., 20.)
mn = min(g, 0.5)
mx = max(0.5, g)
mnt = min(g, 1. - g)
",
    );
    let px0 = |name: &str| get_tex(&ctx, name).as_dense()[0];
    assert!((px0("add_s") - 1.25).abs() < 1e-6);
    assert!((px0("sub_s") - 0.).abs() < 1e-6);
    assert!((px0("rsub_s") - 0.75).abs() < 1e-6);
    assert!((px0("div_s") - 0.125).abs() < 1e-6);
    assert!((px0("rdiv_s") - 4.).abs() < 1e-6);
    assert!((px0("sub_t") - 0.).abs() < 1e-6);
    assert!((px0("div_t") - 0.25 / 1.25).abs() < 1e-6);
    let tint = get_tex(&ctx, "tint").as_dense();
    assert_eq!(tint[0..3], [0.25, 1., 1.]);
    assert_eq!(get_tex(&ctx, "tint2").as_dense()[0..3], [0.25, 1., 1.]);
    assert_eq!(get_tex(&ctx, "vadd").as_dense()[0..3], [1.25, 0.5, 2.]);
    assert_eq!(get_tex(&ctx, "vsub").as_dense()[0..3], [0.75, 0.5, -1.]);
    assert_eq!(get_tex(&ctx, "vdiv").as_dense()[0..3], [0.125, 0.25, 1.]);
    assert!((px0("p") - 0.0625).abs() < 1e-6);
    assert!((px0("cl") - 0.3).abs() < 1e-6);
    assert!((px0("ab") - 0.25).abs() < 1e-6);
    let t = 0.25f32;
    assert!((px0("ss") - t * t * (3. - 2. * t)).abs() < 1e-6);
    assert!((px0("rm") - 12.5).abs() < 1e-6);
    assert!((px0("mn") - 0.25).abs() < 1e-6);
    assert!((px0("mx") - 0.5).abs() < 1e-6);
    assert!((px0("mnt") - 0.25).abs() < 1e-6);

    let err =
      parse_and_eval_program("texture(2, 2, |uv| v2(uv.x, uv.y)) * v3(1., 1., 1.)").unwrap_err();
    assert!(err.to_string().contains("3-channel"), "{err}");
  }

  #[test]
  fn reductions() {
    let ctx = eval(
      "
g = texture(4, 4, |uv| uv.x)
lo = texture_min(g)
hi = texture_max(g)
avg = texture_mean(g)
v = texture_mean(texture(2, 2, |uv| v3(1., 2., 3.)))
",
    );
    assert!((get_f(&ctx, "lo") - 0.125).abs() < 1e-6);
    assert!((get_f(&ctx, "hi") - 0.875).abs() < 1e-6);
    assert!((get_f(&ctx, "avg") - 0.5).abs() < 1e-6);
    match ctx.get_global("v").unwrap() {
      Value::Vec3(v) => assert_eq!((v.x, v.y, v.z), (1., 2., 3.)),
      other => panic!("expected vec3, got {other:?}"),
    }
  }

  /// The image-op builtins that replaced the old `stdlib.geo` prelude, exercised in the
  /// pipeline form the prelude closures got wrong (texture must be the last arg).
  #[test]
  fn image_op_builtins() {
    let ctx = crate::EvalCtx::default();
    crate::parse_and_eval_program_with_ctx(
      "
g = texture(8, 8, |uv| uv.x)
c = g | crop(2, 1, 4, 3)
inv = g | texture_invert
norm = (g * 0.5 + 0.25) | texture_normalize
flat = texture(4, 4, |uv| 0.5) | texture_normalize
imp = texture(8, 8, |uv| 1. - min(1., floor(uv.x * 8.) + floor(uv.y * 8.)))
opened = imp | morph_open(1)
closed = imp | morph_close(1)
grad = imp | morph_outline(1)
th = imp | morph_tophat(1)
bh = (1. - imp) | morph_blackhat(1)
sh = g | sharpen(amt=0.3)
sh2 = sharpen(g, amt=0.3, sigma=1.)
"
      .to_owned(),
      &ctx,
      true,
    )
    .unwrap();
    let g = get_tex(&ctx, "g");
    let gd = g.as_dense();
    let c = get_tex(&ctx, "c");
    assert_eq!((c.width, c.height), (4, 3));
    assert_eq!(c.as_dense()[0], gd[8 + 2]);
    assert!(!c.is_dense(), "crop must stay a view");
    let inv = get_tex(&ctx, "inv");
    assert!((inv.as_dense()[0] - (1. - gd[0])).abs() < 1e-6);
    let nd = get_tex(&ctx, "norm").as_dense();
    assert!((nd[0] - 0.).abs() < 1e-6 && (nd[7] - 1.).abs() < 1e-6);
    // A constant channel has no range to stretch; it must map to 0, not NaN.
    assert!(get_tex(&ctx, "flat").as_dense().iter().all(|&v| v == 0.));
    // Opening removes the lone impulse; closing preserves it.
    assert!(get_tex(&ctx, "opened").as_dense().iter().all(|&v| v == 0.));
    assert_eq!(get_tex(&ctx, "closed").as_dense()[0], 1.);
    // Morphological gradient of an impulse: a 3x3 ring plus center.
    assert_eq!(
      get_tex(&ctx, "grad").as_dense().iter().filter(|&&v| v == 1.).count(),
      9
    );
    assert_eq!(get_tex(&ctx, "th").as_dense()[0], 1.);
    // Blackhat is tophat's dual: on the inverted impulse it isolates the same texel.
    assert_eq!(get_tex(&ctx, "bh").as_dense()[0], 1.);
    assert_eq!(get_tex(&ctx, "sh").width, 8);
    assert_eq!(get_tex(&ctx, "sh2").width, 8);

    // Multi-channel normalize exercises the per-channel path.
    let ctx = crate::EvalCtx::default();
    crate::parse_and_eval_program_with_ctx(
      "n3 = texture_normalize(texture(4, 4, |uv| v3(uv.x, uv.y * 2., 5.5 + uv.x)))".to_owned(),
      &ctx,
      true,
    )
    .unwrap();
    let nd = get_tex(&ctx, "n3").as_dense();
    let (mut mn, mut mx) = ([1f32; 3], [0f32; 3]);
    for px in nd.chunks(3) {
      for c in 0..3 {
        mn[c] = mn[c].min(px[c]);
        mx[c] = mx[c].max(px[c]);
      }
    }
    for c in 0..3 {
      assert!(mn[c].abs() < 1e-5 && (mx[c] - 1.).abs() < 1e-5, "{mn:?} {mx:?}");
    }
  }

  /// Blit a minified view: mips must be built from materialized view pixels (keyed on
  /// the base ptr) rather than read through stale dense indexing.
  #[test]
  fn blit_minified_view_uses_mips() {
    let ctx = eval(
      "
base = texture(2, 2, |uv| 0.)
big = texture(32, 32, |uv| (floor(uv.x * 16.) + floor(uv.y * 16.)) % 2.)
checker = big[0..16, 0..16]
out = blit(checker | scale(0.5) | trans_global(0.25, 0.25), base)
",
    );
    let out = get_tex(&ctx, "out");
    let px = out.as_dense()[0];
    assert!((px - 0.5).abs() < 0.1, "expected ~0.5 from mips, got {px}");
  }
}

