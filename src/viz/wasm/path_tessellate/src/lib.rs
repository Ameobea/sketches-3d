use std::ptr::{addr_of, addr_of_mut};

use lyon_extra::parser::{ParserOptions, PathParser, Source};
use lyon_tessellation::{
  geom::{
    euclid::{Point2D, UnknownUnit},
    Point,
  },
  geometry_builder::Positions,
  path::Path,
  BuffersBuilder, FillOptions, FillRule, FillTessellator, VertexBuffers,
};
use wasm_bindgen::prelude::*;

static mut TESSELLATE_PATH_ERR: String = String::new();

fn set_err(err: String) {
  unsafe {
    let old = std::mem::replace(&mut *addr_of_mut!(TESSELLATE_PATH_ERR), err);
    drop(old);
  }
}

fn get_err() -> &'static String {
  unsafe { &*addr_of!(TESSELLATE_PATH_ERR) }
}

pub struct TessOutput {
  pub vertices: Vec<Point2D<f32, UnknownUnit>>,
  pub indices: Vec<u32>,
}

impl TessOutput {
  pub fn scale(&mut self, width: f32, height: f32) {
    if width <= 0. && height <= 0. {
      return;
    }

    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for v in &self.vertices {
      if v.x < min_x {
        min_x = v.x;
      }
      if v.x > max_x {
        max_x = v.x;
      }
      if v.y < min_y {
        min_y = v.y;
      }
      if v.y > max_y {
        max_y = v.y;
      }
    }

    let measured_width = max_x - min_x;
    let measured_height = max_y - min_y;
    let aspect_ratio = measured_width / measured_height;

    let (scale_x, scale_y) = if width > 0. && height > 0. {
      // ignore existing aspect ratio and scale to fit exactly
      (width / measured_width, height / measured_height)
    } else if width > 0. {
      let scaled_height = width / aspect_ratio;
      (width / measured_width, scaled_height / measured_height)
    } else if height > 0. {
      let scaled_width = height * aspect_ratio;
      (scaled_width / measured_width, height / measured_height)
    } else {
      // technically unreachable due to earlier check
      return;
    };

    for v in &mut self.vertices {
      v.x = (v.x - min_x) * scale_x;
      v.y = (v.y - min_y) * scale_y;
    }
  }
}

#[wasm_bindgen]
pub fn take_tess_output_verts(output: *mut TessOutput) -> Vec<f32> {
  let out = unsafe { &mut *output };
  let verts = std::mem::take(&mut out.vertices);
  verts.into_iter().flat_map(|p| [p.x, p.y]).collect()
}

#[wasm_bindgen]
pub fn take_tess_output_indices(output: *mut TessOutput) -> Vec<u32> {
  let out = unsafe { &mut *output };
  std::mem::take(&mut out.indices)
}

#[wasm_bindgen]
pub fn free_tess_output(output: *mut TessOutput) {
  if output.is_null() {
    return;
  }
  unsafe {
    drop(Box::from_raw(output));
  }
}

fn parse_svg_path(path: &str) -> Result<Path, String> {
  let mut builder = Path::builder();

  let parse_opts = ParserOptions::DEFAULT;
  let res = PathParser::new().parse(&parse_opts, &mut Source::new(path.chars()), &mut builder);
  match res {
    Err(err) => return Err(format!("Error parsing SVG path: {err:?}")),
    Ok(()) => (),
  }

  Ok(builder.build())
}

#[wasm_bindgen]
pub fn tessellate_path(path: &str, width: f32, height: f32) -> *mut TessOutput {
  let path = match parse_svg_path(path) {
    Ok(p) => p,
    Err(err) => {
      set_err(err);
      return std::ptr::null_mut();
    }
  };

  let mut buffers: VertexBuffers<Point<f32>, u32> = VertexBuffers::new();
  let res = {
    let mut vertex_builder = BuffersBuilder::new(&mut buffers, Positions);

    let mut tessellator = FillTessellator::new();

    tessellator.tessellate_path(
      &path,
      &FillOptions::default().with_fill_rule(FillRule::NonZero),
      &mut vertex_builder,
    )
  };
  match res {
    Ok(_) => {
      let mut output = TessOutput {
        vertices: buffers.vertices,
        indices: buffers.indices,
      };
      output.scale(width, height);
      Box::into_raw(Box::new(output))
    }
    Err(err) => {
      set_err(format!("Tessellation error: {err:?}"));
      std::ptr::null_mut()
    }
  }
}

#[wasm_bindgen]
pub fn get_tessellate_path_error() -> String {
  get_err().clone()
}
