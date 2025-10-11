use std::path::Path;

// static UTAH_TEAPOT_DATA: &str = include_str!("data/utah_teapot.norm");
static UTAH_TEAPOT_OBJ_DATA: &str = include_str!("data/teapot.obj");
static STANFORD_BUNNY_OBJ_DATA: &str = include_str!("data/bunny_manifold.obj");
static SUZANNE_OBJ_DATA: &str = include_str!("data/suzanne.obj");

// fn parse_norm(data: &str) -> Vec<[f32; 3]> {
//   // face_count
//   //
//   // v0_x v0_y v0_z
//   // n0_x n0_y n0_z
//   // v1_x v1_y v1_z
//   // n1_x n1_y n1_z
//   // ...

//   let mut lines = data.lines().filter(|l| !l.is_empty());
//   let face_count: usize = lines.next().unwrap().parse().unwrap();

//   let mut vertices = Vec::new();
//   loop {
//     let v_line = match lines.next() {
//       Some(l) => l,
//       None => break,
//     };
//     let mut v_parts = v_line.split_whitespace();
//     let x: f32 = v_parts.next().unwrap().parse().unwrap();
//     let y: f32 = v_parts.next().unwrap().parse().unwrap();
//     let z: f32 = v_parts.next().unwrap().parse().unwrap();
//     vertices.push([x, y, z]);

//     if lines.next().is_none() {
//       break;
//     }
//   }

//   assert_eq!(vertices.len(), face_count * 3);

//   vertices
// }

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
  // let vertices = parse_norm(UTAH_TEAPOT_DATA);
  let (vertices, indices) = parse_obj(UTAH_TEAPOT_OBJ_DATA);

  let out_dir = std::env::var_os("OUT_DIR").unwrap();
  let dest_path = Path::new(&out_dir).join("utah_teapot.rs");

  std::fs::write(
    dest_path,
    format!(
      "pub const UTAH_TEAPOT_VERTICES: &[[f32; 3]] = &{:?};\npub const UTAH_TEAPOT_INDICES: \
       &[u16] = &{:?};\n",
      vertices, indices
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
      "pub const STANFORD_BUNNY_VERTICES: &[[f32; 3]] = &{:?};\npub const STANFORD_BUNNY_INDICES: \
       &[u16] = &{:?};\n",
      vertices, indices
    ),
  )
  .unwrap();

  let (vertices, indices) = parse_obj(SUZANNE_OBJ_DATA);
  let dest_path = Path::new(&out_dir).join("suzanne.rs");
  std::fs::write(
    dest_path,
    format!(
      "pub const SUZANNE_VERTICES: &[[f32; 3]] = &{:?};\npub const SUZANNE_INDICES: &[u16] = \
       &{:?};\n",
      vertices, indices
    ),
  )
  .unwrap();

  println!("cargo:rerun-if-changed=build.rs");
  println!("cargo:rerun-if-changed=data/utah_teapot.norm");
  println!("cargo:rerun-if-changed=data/teapot.obj");
  println!("cargo:rerun-if-changed=data/bunny.obj");
  println!("cargo:rerun-if-changed=data/bunny_manifold.obj");
  println!("cargo:rerun-if-changed=data/suzanne.obj");
}
