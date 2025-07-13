#[derive(Debug, Hash, PartialEq)]
pub enum Material {
  External(String),
}

impl Default for Material {
  fn default() -> Self {
    Material::External(String::new())
  }
}
