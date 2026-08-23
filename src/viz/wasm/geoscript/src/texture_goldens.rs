//! Golden regression net for the texture subsystem (docs/texture-autovec-plan.md Step 0).
//!
//! Each `fixtures/texture/*.geo` program publishes outputs via `render_texture` /
//! `render_texture_stack`; every output slice is recorded as dims + an FNV-1a hash of the
//! raw f32 bits + mean/min/max, and compared line-for-line against `goldens.txt`.
//!
//! Goldens are native-only by decision: the property that matters is same-target A/B
//! (old-vs-new code, later scalar-vs-vectorized), never native-vs-wasm — cross-target
//! bit-equality is impossible anyway (different libm). Regenerate with
//! `UPDATE_GOLDENS=1 cargo test -p geoscript texture_golden`.

use std::fmt::Write as _;
use std::path::PathBuf;

use crate::{parse_and_eval_program_with_ctx, EvalCtx, TextureHandle};

fn record(out: &mut String, fixture: &str, name: &str, slice: usize, t: &TextureHandle) {
  let px = t.as_interleaved();
  let mut h: u64 = 0xcbf29ce484222325;
  for v in px.iter() {
    for b in v.to_le_bytes() {
      h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
  }
  let (mut mean, mut mn, mut mx) = (0f64, f32::INFINITY, f32::NEG_INFINITY);
  for &v in px.iter() {
    mean += v as f64;
    mn = mn.min(v);
    mx = mx.max(v);
  }
  mean /= px.len() as f64;
  writeln!(
    out,
    "{fixture} {name}[{slice}] {}x{}x{} hash={h:016x} mean={mean:.6} min={mn:.6} max={mx:.6}",
    t.width, t.height, t.channels
  )
  .unwrap();
}

#[test]
fn texture_golden_corpus() {
  let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/texture");
  let mut fixtures: Vec<PathBuf> = std::fs::read_dir(&dir)
    .expect("fixtures/texture missing")
    .filter_map(|e| {
      let p = e.unwrap().path();
      (p.extension()? == "geo").then_some(p)
    })
    .collect();
  fixtures.sort();
  assert!(!fixtures.is_empty());

  let mut got = String::new();
  for path in &fixtures {
    let fixture = path.file_stem().unwrap().to_str().unwrap();
    let src = std::fs::read_to_string(path).unwrap();
    // `verify` runs the scalar loop alongside every vectorized texel body and asserts
    // bit-equality, so the corpus proves scalar-vs-vectorized parity on its own rather
    // than just pinning whatever the vectorized path happened to produce.
    let ctx = EvalCtx::default();
    ctx.tex_vectorize.verify.set(true);
    ctx.tex_vectorize.no_vectorize.set(false);
    parse_and_eval_program_with_ctx(src, &ctx, false)
      .unwrap_or_else(|err| panic!("fixture {fixture} failed to eval:\n{err:?}"));
    let rendered = ctx.rendered_textures.into_inner();
    assert!(
      !rendered.is_empty(),
      "fixture {fixture} rendered no textures"
    );
    for rt in &rendered {
      record(&mut got, fixture, &rt.name, 0, &rt.texture);
      for (i, s) in rt.extra_slices.iter().enumerate() {
        record(&mut got, fixture, &rt.name, i + 1, s);
      }
    }
  }

  let golden_path = dir.join("goldens.txt");
  if std::env::var("UPDATE_GOLDENS").is_ok() {
    std::fs::write(&golden_path, &got).unwrap();
    return;
  }
  let want = std::fs::read_to_string(&golden_path)
    .expect("goldens.txt missing; run with UPDATE_GOLDENS=1 to create");
  if got != want {
    let mut msg = String::from("texture goldens mismatch:\n");
    let (gl, wl): (Vec<_>, Vec<_>) = (got.lines().collect(), want.lines().collect());
    for i in 0..gl.len().max(wl.len()) {
      let (g, w) = (
        gl.get(i).copied().unwrap_or("<missing>"),
        wl.get(i).copied().unwrap_or("<missing>"),
      );
      if g != w {
        writeln!(msg, "  got:  {g}\n  want: {w}").unwrap();
      }
    }
    msg.push_str("re-record intentional changes with UPDATE_GOLDENS=1");
    panic!("{msg}");
  }
}
