// Force a rebuild when migrations change; otherwise `sqlx::migrate!` bakes a stale set into the
// binary.
fn main() {
  println!("cargo:rerun-if-changed=migrations");
}
