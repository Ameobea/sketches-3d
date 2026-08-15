//! Color space conversions. Geoscript vec3 colors are linear sRGB; everything here
//! converts to/from that. OKLAB matrices per Björn Ottosson's reference (which take
//! LINEAR sRGB — feeding gamma-encoded values into them is the classic mistake).

use mesh::linked_mesh::Vec3;

pub fn srgb_channel_to_linear(c: f32) -> f32 {
  if c <= 0.04045 {
    c / 12.92
  } else {
    ((c + 0.055) / 1.055).powf(2.4)
  }
}

pub fn linear_channel_to_srgb(c: f32) -> f32 {
  if c <= 0.003_130_8 {
    c * 12.92
  } else {
    1.055 * c.powf(1. / 2.4) - 0.055
  }
}

pub fn srgb_to_linear(c: Vec3) -> Vec3 {
  c.map(srgb_channel_to_linear)
}

pub fn linear_to_srgb(c: Vec3) -> Vec3 {
  c.map(linear_channel_to_srgb)
}

pub fn linear_to_oklab(c: Vec3) -> Vec3 {
  let l = 0.4122214708 * c.x + 0.5363325363 * c.y + 0.0514459929 * c.z;
  let m = 0.2119034982 * c.x + 0.6806995451 * c.y + 0.1073969566 * c.z;
  let s = 0.0883024619 * c.x + 0.2817188376 * c.y + 0.6299787005 * c.z;
  let (l, m, s) = (l.cbrt(), m.cbrt(), s.cbrt());
  Vec3::new(
    0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
  )
}

pub fn oklab_to_linear(c: Vec3) -> Vec3 {
  let l = c.x + 0.3963377774 * c.y + 0.2158037573 * c.z;
  let m = c.x - 0.1055613458 * c.y - 0.0638541728 * c.z;
  let s = c.x - 0.0894841775 * c.y - 1.2914855480 * c.z;
  let (l, m, s) = (l * l * l, m * m * m, s * s * s);
  Vec3::new(
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
  )
}

#[test]
fn oklab_roundtrip_and_known_values() {
  for c in [
    Vec3::new(0., 0., 0.),
    Vec3::new(1., 1., 1.),
    Vec3::new(0.5, 0.2, 0.8),
    Vec3::new(0.01, 0.99, 0.3),
  ] {
    let rt = oklab_to_linear(linear_to_oklab(c));
    assert!((rt - c).norm() < 1e-4, "{c:?} -> {rt:?}");
  }
  // White is L=1 a=b=0; linear mid-gray g has L = cbrt(g).
  let white = linear_to_oklab(Vec3::new(1., 1., 1.));
  assert!((white.x - 1.).abs() < 1e-3 && white.y.abs() < 1e-3 && white.z.abs() < 1e-3);
  assert!((linear_to_oklab(Vec3::new(0.125, 0.125, 0.125)).x - 0.5).abs() < 1e-3);
}
