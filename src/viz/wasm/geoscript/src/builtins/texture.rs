use std::rc::Rc;

use fxhash::FxHashMap;

use crate::{
  ArgRef, ErrorStack, EvalCtx, RenderedTexture, Sym, TextureHandle, TextureWrap, Value, Vec2,
  EMPTY_KWARGS,
};

const MAX_TEXTURE_DIM: i64 = 8192;

impl TextureHandle {
  fn wrap_coord(&self, c: i64, n: usize) -> usize {
    let n = n as i64;
    (match self.wrap {
      TextureWrap::Repeat => c.rem_euclid(n),
      TextureWrap::Clamp => c.clamp(0, n - 1),
      TextureWrap::Mirror => {
        let m = c.rem_euclid(2 * n);
        if m < n {
          m
        } else {
          2 * n - 1 - m
        }
      }
    }) as usize
  }

  pub(crate) fn texel(&self, x: i64, y: i64, chan: usize) -> f32 {
    let x = self.wrap_coord(x, self.width);
    let y = self.wrap_coord(y, self.height);
    self.pixels[(y * self.width + x) * self.channels + chan]
  }
}

pub(crate) fn texture_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let width = arg_refs[0].resolve(args, kwargs).as_int().unwrap();
  let height = arg_refs[1].resolve(args, kwargs).as_int().unwrap();
  let generator = arg_refs[2].resolve(args, kwargs).as_callable().unwrap();
  let wrap = TextureWrap::from_name(arg_refs[3].resolve(args, kwargs).as_str().unwrap())?;

  if width < 1 || height < 1 || width > MAX_TEXTURE_DIM || height > MAX_TEXTURE_DIM {
    return Err(ErrorStack::new(format!(
      "Invalid texture dims {width}x{height}; expected 1..={MAX_TEXTURE_DIM} per side"
    )));
  }
  let (w, h) = (width as usize, height as usize);

  let mut pixels: Vec<f32> = Vec::new();
  let mut channels = 0usize;
  for y in 0..h {
    for x in 0..w {
      let uv = Vec2::new((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
      let out = ctx
        .invoke_callable(generator, &[Value::Vec2(uv)], EMPTY_KWARGS)
        .map_err(|err| {
          err.wrap("Error produced by user-supplied `generator` callable in `texture`")
        })?;
      let px: [f32; 3];
      let n = match &out {
        Value::Vec3(v) => {
          px = [v.x, v.y, v.z];
          3
        }
        v => match v.as_float() {
          Some(f) => {
            px = [f, 0., 0.];
            1
          }
          None => {
            return Err(ErrorStack::new(format!(
              "Expected float or vec3 from `generator` callable in `texture`, found: {out:?}"
            )))
          }
        },
      };
      if channels == 0 {
        channels = n;
        pixels.reserve_exact(w * h * n);
      } else if n != channels {
        return Err(ErrorStack::new(
          "`generator` callable in `texture` returned a mix of float and vec3 values",
        ));
      }
      pixels.extend_from_slice(&px[..n]);
    }
  }

  Ok(Value::Texture(Rc::new(TextureHandle {
    pixels: Rc::new(pixels),
    width: w,
    height: h,
    channels,
    wrap,
  })))
}

pub(crate) fn blur_impl(
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let sigma = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
  let tex_val = arg_refs[1].resolve(args, kwargs);
  let tex = tex_val.as_texture().unwrap();
  if sigma <= 0. {
    return Ok(tex_val.clone());
  }

  let (w, h, ch) = (tex.width, tex.height, tex.channels);
  let half = ((sigma * 3.).ceil() as i64).max(1);
  let mut weights = Vec::with_capacity(half as usize + 1);
  for i in 0..=half {
    weights.push((-((i * i) as f32) / (2. * sigma * sigma)).exp());
  }
  let norm = weights[0] + 2. * weights[1..].iter().sum::<f32>();
  for wt in &mut weights {
    *wt /= norm;
  }

  let pass = |src: &TextureHandle, dx: i64, dy: i64| -> Vec<f32> {
    let px: &[f32] = &src.pixels;
    let stride = (if dx == 1 { 1 } else { w }) * ch;
    let lim = (if dx == 1 { w } else { h }) as i64;
    let mut out = vec![0f32; w * h * ch];
    for y in 0..h {
      for x in 0..w {
        let base = (y * w + x) * ch;
        let pos = (if dx == 1 { x } else { y }) as i64;
        if pos >= half && pos + half < lim {
          // Interior: no tap can wrap, so index directly instead of paying
          // `wrap_coord`'s rem_euclid + branch per tap.
          for c in 0..ch {
            let mut acc = weights[0] * px[base + c];
            for i in 1..=half as usize {
              acc += weights[i] * (px[base + c - i * stride] + px[base + c + i * stride]);
            }
            out[base + c] = acc;
          }
        } else {
          for c in 0..ch {
            let mut acc = weights[0] * src.texel(x as i64, y as i64, c);
            for i in 1..=half {
              acc += weights[i as usize]
                * (src.texel(x as i64 - i * dx, y as i64 - i * dy, c)
                  + src.texel(x as i64 + i * dx, y as i64 + i * dy, c));
            }
            out[base + c] = acc;
          }
        }
      }
    }
    out
  };

  let mid = TextureHandle {
    pixels: Rc::new(pass(tex, 1, 0)),
    width: w,
    height: h,
    channels: ch,
    wrap: tex.wrap,
  };
  Ok(Value::Texture(Rc::new(TextureHandle {
    pixels: Rc::new(pass(&mid, 0, 1)),
    ..mid
  })))
}

pub(crate) fn height_to_normal_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let (strength, tex) = match def_ix {
    0 => (1., arg_refs[0].resolve(args, kwargs).as_texture().unwrap()),
    1 => (
      arg_refs[0].resolve(args, kwargs).as_float().unwrap(),
      arg_refs[1].resolve(args, kwargs).as_texture().unwrap(),
    ),
    _ => unimplemented!(),
  };

  let (w, h) = (tex.width, tex.height);
  let mut out = Vec::with_capacity(w * h * 3);
  for y in 0..h {
    for x in 0..w {
      let (x, y) = (x as i64, y as i64);
      let dx = (tex.texel(x + 1, y, 0) - tex.texel(x - 1, y, 0)) * 0.5 * strength;
      let dy = (tex.texel(x, y + 1, 0) - tex.texel(x, y - 1, 0)) * 0.5 * strength;
      let inv_len = 1. / (dx * dx + dy * dy + 1.).sqrt();
      out.extend_from_slice(&[
        -dx * inv_len * 0.5 + 0.5,
        -dy * inv_len * 0.5 + 0.5,
        inv_len * 0.5 + 0.5,
      ]);
    }
  }

  Ok(Value::Texture(Rc::new(TextureHandle {
    pixels: Rc::new(out),
    width: w,
    height: h,
    channels: 3,
    wrap: tex.wrap,
  })))
}

pub(crate) fn render_texture_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let tex = arg_refs[0].resolve(args, kwargs).as_texture().unwrap();
  let name = arg_refs[1].resolve(args, kwargs).as_str().unwrap();
  ctx.rendered_textures.push(RenderedTexture {
    texture: Rc::clone(tex),
    name: name.to_owned(),
    source_module: ctx.current_module.borrow().clone(),
    texture_id: ctx.next_render_id(),
  });
  Ok(Value::Nil)
}

#[cfg(test)]
mod tests {
  use crate::{
    eval_program_with_ctx, optimizer::optimize_ast, parse_and_eval_program, parse_program_src,
    EvalCtx,
  };

  #[test]
  fn texture_generator_and_render() {
    let ctx = parse_and_eval_program(r#"texture(4, 2, |uv| uv.x) | render_texture(name="height")"#)
      .unwrap();
    let rendered = ctx.rendered_textures.into_inner();
    assert_eq!(rendered.len(), 1);
    let t = &rendered[0];
    assert_eq!(t.name, "height");
    let tex = &t.texture;
    assert_eq!((tex.width, tex.height, tex.channels), (4, 2, 1));
    assert_eq!(tex.pixels.len(), 8);
    for y in 0..2 {
      for x in 0..4 {
        assert_eq!(tex.pixels[y * 4 + x], (x as f32 + 0.5) / 4.);
      }
    }
  }

  #[test]
  fn blur_wrap_modes_and_flat_normals() {
    // Impulse at pixel (0,0): 1 - min(1, floor(u*8) + floor(v*8))
    let src = r#"
gen = |uv| 1. - min(1., floor(uv.x * 8.) + floor(uv.y * 8.))
texture(8, 8, gen) | blur(1.) | render_texture(name="wrapped")
texture(8, 8, gen, wrap="clamp") | blur(1.) | render_texture(name="clamped")
texture(4, 4, |uv| 0.5) | height_to_normal | render_texture(name="flat_n")
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let rendered = ctx.rendered_textures.into_inner();
    assert_eq!(rendered.len(), 3);

    // Repeat wrap leaks the impulse across the seam to (7,0); clamp doesn't.
    let wrapped = &rendered[0].texture;
    let clamped = &rendered[1].texture;
    assert!(wrapped.pixels[7] > 0.);
    assert_eq!(clamped.pixels[7], 0.);

    let flat_n = &rendered[2].texture;
    assert_eq!(flat_n.channels, 3);
    for px in flat_n.pixels.chunks(3) {
      assert_eq!(px, &[0.5, 0.5, 1.0]);
    }
  }

  /// `render_texture` pushes to `rendered_textures` as a side effect, so it must not be
  /// const-folded (same regression class as `path_render`).
  #[test]
  fn render_texture_survives_rerun() {
    let ctx = EvalCtx::default();
    let src = "texture(2, 2, |uv| uv.x) | render_texture";
    let mut ast = parse_program_src(&ctx, src).unwrap();

    for run in 1..=2 {
      ctx.rendered_textures.inner.borrow_mut().clear();
      optimize_ast(&ctx, &mut ast).unwrap();
      eval_program_with_ctx(&ctx, &ast).unwrap();
      assert_eq!(
        ctx.rendered_textures.len(),
        1,
        "expected a rendered texture on run {run}"
      );
    }
  }
}
