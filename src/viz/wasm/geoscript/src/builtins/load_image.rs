//! `load_image`: decodes a base64 data-URI image into a float texture. On wasm the
//! decode happens host-side (browser `createImageBitmap`) via the `image_data` async-dep
//! extern; native builds decode PNG directly so tests cover the full conversion path.

use std::rc::Rc;

use fxhash::FxHashMap;

use crate::color::srgb_channel_to_linear;
use crate::{ArgRef, ErrorStack, EvalCtx, Mat4, Sym, TextureHandle, TextureWrap, Value};

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(module = "src/geoscript/imageData")]
extern "C" {
  fn image_data_is_loaded(uri: &str) -> bool;
  fn image_data_get_dims(uri: &str) -> Vec<u32>;
  fn image_data_get_rgba(uri: &str) -> Vec<u8>;
}

#[cfg(target_arch = "wasm32")]
fn fetch_rgba(uri: &str) -> Result<(usize, usize, Vec<u8>), ErrorStack> {
  crate::or_async_dep_bit(crate::DEP_BIT_IMAGE_DATA);
  if !image_data_is_loaded(uri) {
    return Err(ErrorStack::new_uninitialized_module_with_args(
      "image_data",
      std::iter::once(uri.to_owned()),
    ));
  }
  let dims = image_data_get_dims(uri);
  Ok((dims[0] as usize, dims[1] as usize, image_data_get_rgba(uri)))
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_rgba(uri: &str) -> Result<(usize, usize, Vec<u8>), ErrorStack> {
  use base64::Engine;

  let b64 = uri.strip_prefix("data:image/png;base64,").ok_or_else(|| {
    ErrorStack::new("native `load_image` supports only `data:image/png;base64,` URIs")
  })?;
  let bytes = base64::engine::general_purpose::STANDARD
    .decode(b64)
    .map_err(|err| ErrorStack::new(format!("invalid base64 in data URI: {err}")))?;
  let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
  let mut reader = decoder
    .read_info()
    .map_err(|err| ErrorStack::new(format!("error decoding PNG: {err}")))?;
  let mut buf = vec![0u8; reader.output_buffer_size()];
  let info = reader
    .next_frame(&mut buf)
    .map_err(|err| ErrorStack::new(format!("error decoding PNG: {err}")))?;
  if info.bit_depth != png::BitDepth::Eight {
    return Err(ErrorStack::new("only 8-bit PNGs are supported"));
  }
  let (w, h) = (info.width as usize, info.height as usize);
  let mut rgba = Vec::with_capacity(w * h * 4);
  match info.color_type {
    png::ColorType::Grayscale => {
      for &v in &buf[..w * h] {
        rgba.extend_from_slice(&[v, v, v, 255]);
      }
    }
    png::ColorType::GrayscaleAlpha => {
      for px in buf[..w * h * 2].chunks_exact(2) {
        rgba.extend_from_slice(&[px[0], px[0], px[0], px[1]]);
      }
    }
    png::ColorType::Rgb => {
      for px in buf[..w * h * 3].chunks_exact(3) {
        rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
      }
    }
    png::ColorType::Rgba => rgba.extend_from_slice(&buf[..w * h * 4]),
    other => {
      return Err(ErrorStack::new(format!(
        "unsupported PNG color type: {other:?}"
      )))
    }
  }
  Ok((w, h, rgba))
}

pub(crate) fn load_image_impl(
  _ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let uri_v = arg_refs[0].resolve(args, kwargs);
  let uri = uri_v.as_str().unwrap();
  if !uri.starts_with("data:image/") {
    return Err(ErrorStack::new(
      "`load_image` expects a `data:image/...;base64,...` data URI",
    ));
  }
  let srgb = arg_refs[1].resolve(args, kwargs).as_bool().unwrap();
  let scale = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
  let offset = arg_refs[3].resolve(args, kwargs).as_float().unwrap();
  let channels_v = arg_refs[4].resolve(args, kwargs);
  let forced_channels = if channels_v.is_nil() {
    None
  } else {
    match channels_v.as_int() {
      Some(c @ (1 | 3 | 4)) => Some(c as usize),
      _ => {
        return Err(ErrorStack::new(format!(
          "`channels` must be 1, 3, or 4; found: {channels_v:?}"
        )))
      }
    }
  };

  let (w, h, rgba) = fetch_rgba(uri)?;
  if rgba.len() != w * h * 4 {
    return Err(ErrorStack::new(format!(
      "decoded image data size mismatch: {}x{} but {} bytes",
      w,
      h,
      rgba.len()
    )));
  }

  let channels = forced_channels.unwrap_or_else(|| {
    let has_alpha = rgba.chunks_exact(4).any(|px| px[3] != 255);
    if has_alpha {
      4
    } else if rgba
      .chunks_exact(4)
      .all(|px| px[0] == px[1] && px[1] == px[2])
    {
      1
    } else {
      3
    }
  });

  let conv = |v: u8| -> f32 {
    let u = v as f32 / 255.;
    let u = if srgb { srgb_channel_to_linear(u) } else { u };
    u * scale + offset
  };
  let mut planes: Vec<Vec<f32>> = (0..channels).map(|_| Vec::with_capacity(w * h)).collect();
  for px in rgba.chunks_exact(4) {
    for c in 0..channels.min(3) {
      planes[c].push(conv(px[c]));
    }
    if channels == 4 {
      planes[3].push(px[3] as f32 / 255.);
    }
  }

  Ok(Value::Texture(Rc::new(TextureHandle {
    storage: crate::TexStorage::from_plane_vecs(planes),
    width: w,
    height: h,
    channels,
    wrap: TextureWrap::Repeat,
    min_filter: None,
    mag_filter: None,
    format: None,
    transform: Mat4::identity(),
    mips: Default::default(),
  })))
}

#[cfg(all(not(target_arch = "wasm32"), test))]
mod tests {
  use crate::{parse_and_eval_program, TextureHandle, Value};
  use base64::Engine;
  use std::rc::Rc;

  fn png_data_uri(w: u32, h: u32, color_type: png::ColorType, data: &[u8]) -> String {
    let mut buf = Vec::new();
    {
      let mut enc = png::Encoder::new(&mut buf, w, h);
      enc.set_color(color_type);
      enc.set_depth(png::BitDepth::Eight);
      enc.write_header().unwrap().write_image_data(data).unwrap();
    }
    format!(
      "data:image/png;base64,{}",
      base64::engine::general_purpose::STANDARD.encode(&buf)
    )
  }

  fn get_tex(ctx: &crate::EvalCtx, name: &str) -> Rc<TextureHandle> {
    match ctx.get_global(name).unwrap() {
      Value::Texture(t) => t,
      other => panic!("expected {name} to be a texture, found: {other:?}"),
    }
  }

  #[test]
  fn load_image_gray_scale_offset() {
    let uri = png_data_uri(2, 2, png::ColorType::Grayscale, &[0, 85, 170, 255]);
    let ctx = parse_and_eval_program(&format!(
      "t = load_image(\"{uri}\", srgb=false, scale=2., offset=-1.)"
    ))
    .unwrap();
    let t = get_tex(&ctx, "t");
    assert_eq!((t.width, t.height, t.channels), (2, 2, 1));
    let expected = [-1., 85. / 255. * 2. - 1., 170. / 255. * 2. - 1., 1.];
    for (px, exp) in t.as_interleaved().iter().zip(expected) {
      assert!((px - exp).abs() < 1e-6, "{px} vs {exp}");
    }
  }

  #[test]
  fn load_image_srgb_color_and_channel_forcing() {
    let uri = png_data_uri(1, 1, png::ColorType::Rgb, &[255, 128, 0]);
    let ctx = parse_and_eval_program(&format!(
      "auto = load_image(\"{uri}\")\nforced = load_image(\"{uri}\", channels=4)"
    ))
    .unwrap();
    let auto = get_tex(&ctx, "auto");
    assert_eq!(auto.channels, 3);
    assert!((auto.as_interleaved()[0] - 1.).abs() < 1e-6);
    // sRGB 128/255 decodes to ~0.2158 linear
    assert!(
      (auto.as_interleaved()[1] - 0.2158).abs() < 1e-3,
      "{}",
      auto.as_interleaved()[1]
    );
    assert_eq!(auto.as_interleaved()[2], 0.);

    let forced = get_tex(&ctx, "forced");
    assert_eq!(forced.channels, 4);
    assert_eq!(forced.as_interleaved()[3], 1.);
  }

  #[test]
  fn load_image_alpha_autodetect_and_errors() {
    let uri = png_data_uri(1, 1, png::ColorType::Rgba, &[10, 20, 30, 128]);
    let ctx = parse_and_eval_program(&format!("t = load_image(\"{uri}\", srgb=false)")).unwrap();
    let t = get_tex(&ctx, "t");
    assert_eq!(t.channels, 4);
    assert!((t.as_interleaved()[3] - 128. / 255.).abs() < 1e-6);

    let err = parse_and_eval_program("load_image(\"http://example.com/x.png\")").unwrap_err();
    assert!(err.to_string().contains("data URI"), "{err}");

    let uri = png_data_uri(1, 1, png::ColorType::Grayscale, &[7]);
    let err = parse_and_eval_program(&format!("load_image(\"{uri}\", channels=2)")).unwrap_err();
    assert!(err.to_string().contains("must be 1, 3, or 4"), "{err}");
  }

  /// The texton-scatter idiom end to end: signed stamps accumulated additively with
  /// analytic normalization produce a ~standardized field.
  #[test]
  fn load_image_texton_scatter() {
    // 4x4 kernel, values roughly zero-mean once dequantized via scale/offset
    let kern: [u8; 16] = [
      128, 200, 80, 128, 200, 255, 128, 60, 80, 128, 0, 128, 128, 60, 128, 180,
    ];
    let uri = png_data_uri(4, 4, png::ColorType::Grayscale, &kern);
    let mean_h2 = kern
      .iter()
      .map(|&v| {
        let x = v as f64 / 255. * 2. - 1.;
        x * x
      })
      .sum::<f64>()
      / 16.;
    // Pixel-snapped placement + nearest filter: bilinear sub-pixel stamping would tent-
    // filter the kernel, losing variance and rolling off its spectrum.
    let src = format!(
      r#"
set_rng_seed(7)
kern = load_image("{uri}", srgb=false, scale=2., offset=-1.)
n = 64
s = 4.
cov = 20.
count = int(cov * (n * n) / (s * s))
place = || (floor(randf() * n) + s * 0.5) / n
field = scatter(
  count,
  |ix| kern * (floor(randf() * 2.) * 2. - 1.) | scale(s / n) | trans_global(place(), place()),
  texture(n, n, |uv| 0.),
  blend="add",
  filter="nearest"
) * (1. / sqrt(cov * {mean_h2}))
"#
    );
    let ctx = parse_and_eval_program(&src).unwrap();
    let t = get_tex(&ctx, "field");
    assert_eq!((t.width, t.height, t.channels), (64, 64, 1));
    let n = t.as_interleaved().len() as f64;
    let mean = t.as_interleaved().iter().map(|&v| v as f64).sum::<f64>() / n;
    let var = t
      .as_interleaved()
      .iter()
      .map(|&v| (v as f64 - mean).powi(2))
      .sum::<f64>()
      / n;
    assert!(mean.abs() < 0.15, "mean {mean}");
    assert!((var.sqrt() - 1.).abs() < 0.25, "std {}", var.sqrt());
  }
}
