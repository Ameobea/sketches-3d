use std::str::FromStr;

use fxhash::FxHashMap;
use mesh::linked_mesh::Vec3;
use nalgebra::Matrix4;
use nanoserde::SerJson;

use crate::{ErrorStack, Value};

fn parse_color_value(color: &Value) -> Result<u32, ErrorStack> {
  match color {
    Value::Int(i) => {
      if *i < 0 {
        return Err(ErrorStack::new("Color value cannot be negative"));
      }

      Ok(*i as u32)
    }
    Value::Vec3(v) => {
      if v.x < 0. || v.y < 0. || v.z < 0. {
        return Err(ErrorStack::new("Color vector values should all be [0, 1]"));
      }

      let r = (v.x * 255.).clamp(0., 255.) as u32;
      let g = (v.y * 255.).clamp(0., 255.) as u32;
      let b = (v.z * 255.).clamp(0., 255.) as u32;
      Ok((r << 16) | (g << 8) | b)
    }
    _ => Err(ErrorStack::new("Invalid color value, expected int or vec3")),
  }
}

#[derive(SerJson)]
struct SerializableVec3(Vec<f32>);

impl From<&Vec3> for SerializableVec3 {
  fn from(vec: &Vec3) -> Self {
    Self(vec.as_slice().to_vec())
  }
}

#[derive(SerJson)]
struct SerializableMat4(Vec<f32>);

impl From<&Matrix4<f32>> for SerializableMat4 {
  fn from(mat: &Matrix4<f32>) -> Self {
    Self(mat.as_slice().to_vec())
  }
}

#[derive(Debug, Clone, SerJson)]
pub struct AmbientLight {
  pub color: u32,
  pub intensity: f32,
  #[nserde(proxy = "SerializableMat4")]
  pub transform: Matrix4<f32>,
}

impl AmbientLight {
  pub(crate) fn new(color: &Value, intensity: f32) -> Result<Self, ErrorStack> {
    let color = parse_color_value(color)?;
    if intensity < 0. {
      return Err(ErrorStack::new(
        "Ambient light intensity cannot be negative",
      ));
    }

    Ok(Self {
      color,
      intensity,
      transform: Matrix4::identity(),
    })
  }
}

impl Default for AmbientLight {
  fn default() -> Self {
    Self {
      color: 0xffffff,
      intensity: 1.8,
      transform: Matrix4::identity(),
    }
  }
}

#[derive(Debug, Clone, SerJson)]
pub struct ShadowMapSize {
  pub width: u32,
  pub height: u32,
}

impl ShadowMapSize {
  fn from_map(map: &FxHashMap<String, Value>) -> Result<Self, ErrorStack> {
    let width = map.get("width").and_then(Value::as_int).unwrap_or(2048 * 2);
    if width <= 0 {
      return Err(ErrorStack::new(
        "Shadow map width must be greater than zero",
      ));
    } else if width > 10_000 {
      return Err(ErrorStack::new(
        "Shadow map width size is not reasonable; it must be less than 10,000",
      ));
    }
    let height = map
      .get("height")
      .and_then(Value::as_int)
      .unwrap_or(2048 * 2);
    if height <= 0 {
      return Err(ErrorStack::new(
        "Shadow map height must be greater than zero",
      ));
    } else if height > 10_000 {
      return Err(ErrorStack::new(
        "Shadow map height size is not reasonable; it must be less than 10,000",
      ));
    }

    Ok(ShadowMapSize {
      width: width as u32,
      height: height as u32,
    })
  }
}

impl Default for ShadowMapSize {
  fn default() -> Self {
    Self {
      width: 2048 * 2,
      height: 2048 * 2,
    }
  }
}

#[derive(Debug, Clone, SerJson)]
pub struct ShadowCamera {
  pub near: f32,
  pub far: f32,
  pub left: f32,
  pub right: f32,
  pub top: f32,
  pub bottom: f32,
}

impl ShadowCamera {
  fn from_map(map: &FxHashMap<String, Value>) -> Result<Self, ErrorStack> {
    let mut this = Self::default();
    for (key, val) in map {
      match key.as_str() {
        "near" => {
          this.near = val
            .as_float()
            .ok_or_else(|| ErrorStack::new("Shadow camera 'near' value must be a float"))?;
        }
        "far" => {
          this.far = val
            .as_float()
            .ok_or_else(|| ErrorStack::new("Shadow camera 'far' value must be a float"))?;
        }
        "left" => {
          this.left = val
            .as_float()
            .ok_or_else(|| ErrorStack::new("Shadow camera 'left' value must be a float"))?;
        }
        "right" => {
          this.right = val
            .as_float()
            .ok_or_else(|| ErrorStack::new("Shadow camera 'right' value must be a float"))?;
        }
        "top" => {
          this.top = val
            .as_float()
            .ok_or_else(|| ErrorStack::new("Shadow camera 'top' value must be a float"))?;
        }
        "bottom" => {
          this.bottom = val
            .as_float()
            .ok_or_else(|| ErrorStack::new("Shadow camera 'bottom' value must be a float"))?;
        }
        _ => {
          return Err(ErrorStack::new(format!(
            "Unknown shadow camera parameter: {key}"
          )))
        }
      }
    }

    Ok(this)
  }
}

impl Default for ShadowCamera {
  fn default() -> Self {
    Self {
      near: 8.0,
      far: 300.0,
      left: -300.0,
      right: 380.0,
      top: 94.0,
      bottom: -140.0,
    }
  }
}

#[derive(Debug, Clone, SerJson)]
pub enum ShadowMapType {
  Vsm,
}

impl FromStr for ShadowMapType {
  type Err = ErrorStack;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    match s {
      "vsm" => Ok(ShadowMapType::Vsm),
      _ => Err(ErrorStack::new("Invalid shadow map type")),
    }
  }
}

impl ShadowMapType {
  pub fn to_str(&self) -> &str {
    match self {
      ShadowMapType::Vsm => "vsm",
    }
  }
}

#[derive(Debug, Clone, SerJson)]
pub struct DirectionalLight {
  #[nserde(proxy = "SerializableVec3")]
  pub target: Vec3,
  pub color: u32,
  pub intensity: f32,
  pub cast_shadow: bool,
  pub shadow_map_size: ShadowMapSize,
  pub shadow_map_radius: f32,
  pub shadow_map_blur_samples: u32,
  pub shadow_map_type: ShadowMapType,
  pub shadow_map_bias: f32,
  pub shadow_camera: ShadowCamera,
  #[nserde(proxy = "SerializableMat4")]
  pub transform: Matrix4<f32>,
}

impl DirectionalLight {
  pub(crate) fn new(
    target: &Vec3,
    color: &Value,
    intensity: f32,
    cast_shadow: bool,
    shadow_map_size: &Value,
    shadow_map_radius: f32,
    shadow_map_blur_samples: usize,
    shadow_map_type: &str,
    shadow_map_bias: f32,
    shadow_camera: &FxHashMap<String, Value>,
  ) -> Result<Self, ErrorStack> {
    let color = parse_color_value(color)?;

    let shadow_map_size = match shadow_map_size {
      Value::Map(map) => ShadowMapSize::from_map(map)?,
      Value::Int(size) => ShadowMapSize {
        width: *size as u32,
        height: *size as u32,
      },
      _ => return Err(ErrorStack::new("Invalid shadow map size value")),
    };

    let shadow_camera = ShadowCamera::from_map(shadow_camera)
      .map_err(|err| err.wrap("Invalid shadow camera parameters"))?;

    Ok(Self {
      target: *target,
      color,
      intensity,
      cast_shadow,
      shadow_map_size,
      shadow_map_radius,
      shadow_map_blur_samples: shadow_map_blur_samples as u32,
      shadow_map_type: ShadowMapType::from_str(shadow_map_type)?,
      shadow_map_bias,
      shadow_camera,
      transform: Matrix4::identity(),
    })
  }
}

impl Default for DirectionalLight {
  fn default() -> Self {
    Self {
      target: Vec3::new(0., 0., 0.),
      color: 0xffffff,
      intensity: 5.0,
      cast_shadow: true,
      shadow_map_size: ShadowMapSize::default(),
      shadow_map_radius: 4.0,
      shadow_map_blur_samples: 16,
      shadow_map_type: ShadowMapType::Vsm,
      shadow_map_bias: -0.0001,
      shadow_camera: ShadowCamera::default(),
      transform: Matrix4::identity(),
    }
  }
}

#[derive(Debug, Clone, SerJson)]
pub struct PointLight {
  #[nserde(proxy = "SerializableMat4")]
  pub transform: Matrix4<f32>,
}

#[derive(Clone, Debug, SerJson)]
pub enum Light {
  Ambient(AmbientLight),
  Directional(DirectionalLight),
  Point(PointLight),
}
impl Light {
  pub(crate) fn transform_mut(&mut self) -> &mut Matrix4<f32> {
    match self {
      Light::Ambient(a) => &mut a.transform,
      Light::Directional(d) => &mut d.transform,
      Light::Point(p) => &mut p.transform,
    }
  }
}
