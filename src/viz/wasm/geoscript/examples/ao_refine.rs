use std::time::Instant;

fn main() {
  let args: Vec<String> = std::env::args().collect();
  let src = std::fs::read_to_string(&args[1]).unwrap();
  let t = Instant::now();
  let ctx = geoscript::parse_and_eval_program(src).unwrap();
  let dt = t.elapsed();
  for m in ctx.rendered_meshes.borrow().iter() {
    let m = &m.mesh.mesh;
    let (mut lo, mut hi) = (0usize, 0usize);
    if let Some(ch) = m.vertex_channels.get("ao") {
      for k in m.vertices.keys() {
        let v = ch.get(k).unwrap()[0];
        if v < 0.02 {
          lo += 1
        } else if v > 0.98 {
          hi += 1
        }
      }
    }
    println!(
      "verts {} faces {} edges {} ao<0.02: {} ao>0.98: {} ({:.1} ms)",
      m.vertices.len(),
      m.faces.len(),
      m.edges.len(),
      lo,
      hi,
      dt.as_secs_f64() * 1e3
    );
  }
}
