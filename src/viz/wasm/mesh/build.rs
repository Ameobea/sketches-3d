use std::path::Path;

static UTAH_TEAPOT_OBJ_DATA: &str = include_str!("data/teapot.obj");
static STANFORD_BUNNY_OBJ_DATA: &str = include_str!("data/bunny_manifold.obj");

fn parse_obj(data: &str) -> (Vec<[f32; 3]>, Vec<u32>) {
  let mut vertices = Vec::new();
  let mut indices = Vec::new();

  for line in data.lines() {
    if line.starts_with("v ") {
      let parts: Vec<&str> = line.split_whitespace().collect();
      let x: f32 = parts[1].parse().unwrap();
      let y: f32 = parts[2].parse().unwrap();
      let z: f32 = parts[3].parse().unwrap();
      vertices.push([x, y, z]);
    } else if line.starts_with("f ") {
      let parts: Vec<&str> = line.split_whitespace().collect();
      for part in &parts[1..] {
        let index: u32 = part.split('/').next().unwrap().parse().unwrap();
        indices.push(index - 1);
      }
    }
  }

  (vertices, indices)
}

fn main() {
  let (vertices, indices) = parse_obj(UTAH_TEAPOT_OBJ_DATA);

  let out_dir = std::env::var_os("OUT_DIR").unwrap();
  let dest_path = Path::new(&out_dir).join("utah_teapot.rs");

  std::fs::write(
    dest_path,
    format!(
      "pub const UTAH_TEAPOT_VERTICES: &[[f32; 3]] = &{vertices:?};\npub const \
       UTAH_TEAPOT_INDICES: &[u16] = &{indices:?};\n"
    ),
  )
  .unwrap();

  let (mut vertices, indices) = parse_obj(STANFORD_BUNNY_OBJ_DATA);
  // the stanford bunny scale is very very small by default, so add some default scale to make it
  // more convenient
  for v in &mut vertices {
    for v in v.iter_mut() {
      *v *= 100.;
    }
  }

  let dest_path = Path::new(&out_dir).join("stanford_bunny.rs");
  std::fs::write(
    dest_path,
    format!(
      "pub const STANFORD_BUNNY_VERTICES: &[[f32; 3]] = &{vertices:?};\npub const \
       STANFORD_BUNNY_INDICES: &[u16] = &{indices:?};\n"
    ),
  )
  .unwrap();

  println!("cargo:rerun-if-changed=build.rs");
  println!("cargo:rerun-if-changed=data/teapot.obj");
  println!("cargo:rerun-if-changed=data/bunny.obj");
  println!("cargo:rerun-if-changed=data/bunny_manifold.obj");
}
